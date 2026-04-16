use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::error::LuluError;
use crate::rand_util::generate_random_string;
use crate::serializer::{self, PendingMessage};
use crate::topic;

// ---------------------------------------------------------------------------
// LuluClientConfig
// ---------------------------------------------------------------------------

/// Configuration for the MQTT connection used by `lulu_init()`.
#[derive(Debug, Clone)]
pub struct LuluConfig {
    /// MQTT broker hostname or IP address.
    pub broker_host: String,
    /// MQTT broker TCP port.
    pub broker_port: u16,
    /// Prefix used to generate the MQTT client ID (`{prefix}-{random}`).
    pub client_id_prefix: String,
    /// Maximum number of pending messages in the internal queue.
    pub queue_capacity: usize,
    /// MQTT keep-alive interval in seconds.
    pub keep_alive_secs: u64,
    /// When `true`, test-scenario lifecycle events (`beg_test_scenario` /
    /// `end_test_scenario`) are printed to stdout with coloured status
    /// indicators (green = pass, red = fail). Defaults to `false`.
    pub terminal_logger: bool,
}

impl Default for LuluConfig {
    fn default() -> Self {
        Self {
            broker_host: "127.0.0.1".to_string(),
            broker_port: 1883,
            client_id_prefix: "lulu-logs-client".to_string(),
            queue_capacity: 256,
            keep_alive_secs: 5,
            terminal_logger: false,
        }
    }
}

// ---------------------------------------------------------------------------
// LuluStats
// ---------------------------------------------------------------------------

/// Runtime statistics exposed via `lulu_stats()`.
#[derive(Debug, Clone, Default)]
pub struct LuluStats {
    /// Messages successfully published to the broker.
    pub messages_published: u64,
    /// Messages dropped (queue full, serialization or publish error).
    pub messages_dropped: u64,
    /// Current number of messages waiting in the queue.
    pub queue_current_size: usize,
    /// Number of MQTT reconnections since start.
    pub reconnections: u32,
}

// ---------------------------------------------------------------------------
// LuluClient (crate-internal)
// ---------------------------------------------------------------------------

pub(crate) struct LuluClient {
    sender: mpsc::Sender<PendingMessage>,
    messages_published: Arc<AtomicU64>,
    messages_dropped: Arc<AtomicU64>,
    reconnections: Arc<AtomicU32>,
    connected: Arc<AtomicBool>,
    /// Cloned MQTT client used to publish heartbeat pulses.
    pub(crate) mqtt_client: AsyncClient,
    /// Active pulse tasks keyed by source string (e.g. `"psu/channel-1"`).
    pulse_handles: Mutex<HashMap<String, tokio::task::AbortHandle>>,
}

impl LuluClient {
    /// Starts the MQTT connection and background workers.
    pub(crate) async fn start(config: LuluConfig) -> anyhow::Result<Self> {
        let client_id = format!("{}-{}", config.client_id_prefix, generate_random_string(6));

        let (sender, receiver) = mpsc::channel::<PendingMessage>(config.queue_capacity);

        let messages_published = Arc::new(AtomicU64::new(0));
        let messages_dropped = Arc::new(AtomicU64::new(0));
        let reconnections = Arc::new(AtomicU32::new(0));
        let connected = Arc::new(AtomicBool::new(false));

        let mut mqtt_options = MqttOptions::new(client_id, &config.broker_host, config.broker_port);
        mqtt_options.set_keep_alive(std::time::Duration::from_secs(config.keep_alive_secs));

        let (async_client, event_loop) = AsyncClient::new(mqtt_options, 100);

        tokio::spawn(send_loop(
            receiver,
            async_client.clone(),
            Arc::clone(&messages_published),
            Arc::clone(&messages_dropped),
        ));

        tokio::spawn(mqtt_connection_loop(
            event_loop,
            Arc::clone(&connected),
            Arc::clone(&reconnections),
        ));

        Ok(Self {
            sender,
            messages_published,
            messages_dropped,
            reconnections,
            connected,
            mqtt_client: async_client,
            pulse_handles: Mutex::new(HashMap::new()),
        })
    }

    /// Enqueues a message for publication. Non-blocking.
    pub fn publish(&self, msg: PendingMessage) -> Result<(), LuluError> {
        self.sender.try_send(msg).map_err(|e| {
            self.messages_dropped.fetch_add(1, Ordering::Relaxed);
            match e {
                TrySendError::Full(_) | TrySendError::Closed(_) => LuluError::QueueFull,
            }
        })
    }

    /// Returns a snapshot of the runtime statistics.
    pub fn stats(&self) -> LuluStats {
        LuluStats {
            messages_published: self.messages_published.load(Ordering::Relaxed),
            messages_dropped: self.messages_dropped.load(Ordering::Relaxed),
            queue_current_size: self.sender.max_capacity() - self.sender.capacity(),
            reconnections: self.reconnections.load(Ordering::Relaxed),
        }
    }

    /// Returns whether the MQTT connection is currently alive.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Spawns a background task that publishes a heartbeat on `pulse_topic` every 2 seconds.
    /// The first pulse is emitted immediately (before the first 2-second wait).
    /// Calling again with the same `source` replaces the existing task.
    pub fn start_pulse(
        &self,
        source: String,
        pulse_topic: String,
        version: Option<String>,
        rt: &tokio::runtime::Runtime,
    ) {
        let mqtt = self.mqtt_client.clone();
        let handle = rt.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let ts = chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string();
                let payload = match &version {
                    Some(v) => serde_json::json!({"timestamp": ts, "version": v}).to_string(),
                    None => serde_json::json!({"timestamp": ts}).to_string(),
                };
                if let Err(e) = mqtt
                    .publish(&pulse_topic, QoS::AtMostOnce, false, payload.into_bytes())
                    .await
                {
                    tracing::warn!("pulse publish failed for {}: {}", pulse_topic, e);
                }
            }
        });
        // Replace any existing handle for this source (aborting the old task)
        if let Some(old) = self
            .pulse_handles
            .lock()
            .unwrap()
            .insert(source, handle.abort_handle())
        {
            old.abort();
        }
    }

    /// Stops the heartbeat task for the given source. No-op if none is running.
    pub fn stop_pulse(&self, source: &str) {
        if let Some(handle) = self.pulse_handles.lock().unwrap().remove(source) {
            handle.abort();
        }
    }

    /// Stops all active heartbeat tasks.
    pub fn stop_all_pulses(&self) {
        let mut handles = self.pulse_handles.lock().unwrap();
        for (_, handle) in handles.drain() {
            handle.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// send_loop — background task that drains the queue and publishes to MQTT
// ---------------------------------------------------------------------------

async fn send_loop(
    mut receiver: mpsc::Receiver<PendingMessage>,
    async_client: AsyncClient,
    messages_published: Arc<AtomicU64>,
    messages_dropped: Arc<AtomicU64>,
) {
    while let Some(msg) = receiver.recv().await {
        let topic_str = topic::build_topic(&msg.source_segments, &msg.attribute);

        let payload = match serializer::serialize(&msg) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("serialization failed, dropping message: {}", e);
                messages_dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };

        match async_client
            .publish(&topic_str, QoS::AtMostOnce, false, Bytes::from(payload))
            .await
        {
            Ok(()) => {
                messages_published.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::warn!("mqtt publish failed, dropping message: {}", e);
                messages_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    tracing::info!("send_loop terminated — channel closed");
}

// ---------------------------------------------------------------------------
// mqtt_connection_loop — drives the rumqttc event loop with exp. backoff
// ---------------------------------------------------------------------------

async fn mqtt_connection_loop(
    mut event_loop: EventLoop,
    connected: Arc<AtomicBool>,
    reconnections: Arc<AtomicU32>,
) {
    let mut backoff_ms: u64 = 1_000;
    const MAX_BACKOFF_MS: u64 = 30_000;

    loop {
        match event_loop.poll().await {
            Ok(notification) => {
                // Reset backoff on any successful event
                backoff_ms = 1_000;

                // Detect connection established
                if let rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_)) = notification {
                    let was_connected = connected.swap(true, Ordering::Relaxed);
                    if !was_connected {
                        tracing::info!("mqtt connected");
                    }
                }
            }
            Err(e) => {
                let was_connected = connected.swap(false, Ordering::Relaxed);
                if was_connected {
                    reconnections.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!("mqtt connection lost: {}", e);
                }

                tracing::debug!("mqtt reconnecting in {} ms", backoff_ms);
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;

                // Exponential backoff: 1s → 2s → 4s → 8s → 16s → 30s (capped)
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }
        }
    }
}
