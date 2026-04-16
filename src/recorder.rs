//! # Embedded recorder
//!
//! Starts a local MQTT broker and subscribes to `lulu/#` to capture all log
//! entries published by the current process.  On shutdown the accumulated
//! records are serialised as a `.lulu` FlatBuffers file.  If the target file
//! already exists its existing records are preserved and the new ones are
//! appended.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flatbuffers::FlatBufferBuilder;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};

use crate::lulu_export_generated::lulu_export::{
    root_as_lulu_export_file, LogRecord, LogRecordArgs, LuluExportFile, LuluExportFileArgs,
};
use crate::rand_util::generate_random_string;

// ---------------------------------------------------------------------------
// Record type
// ---------------------------------------------------------------------------

/// A captured MQTT message: the full topic and the raw FlatBuffers payload.
#[derive(Clone)]
pub(crate) struct CapturedRecord {
    pub topic:   String,
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------
// LuluRecorder
// ---------------------------------------------------------------------------

/// Runs an embedded MQTT broker and records all `lulu/#` messages.
pub(crate) struct LuluRecorder {
    /// Accumulated records (shared with the subscriber task).
    records:  Arc<Mutex<Vec<CapturedRecord>>>,
    /// Destination file for the `.lulu` export.
    file_path: PathBuf,
    /// Handle to the subscriber task so we can abort it on stop.
    sub_handle: tokio::task::AbortHandle,
    /// MQTT client used by the subscriber (kept to send `disconnect` on stop).
    sub_client: AsyncClient,
}

impl LuluRecorder {
    /// Starts the embedded broker and the subscriber.
    ///
    /// Returns `(recorder, broker_port)` so the caller can forward the port to
    /// `lulu_init`.
    pub(crate) async fn start(file_path: PathBuf) -> anyhow::Result<(Self, u16)> {
        // ── 1. Find a free TCP port ──────────────────────────────────────────
        let port = free_port()?;

        // ── 2. Start the embedded broker ────────────────────────────────────
        start_broker_thread(port);

        // ── 3. Give the broker a moment to bind before connecting ───────────
        tokio::time::sleep(Duration::from_millis(200)).await;

        // ── 4. Connect the subscriber ────────────────────────────────────────
        let records: Arc<Mutex<Vec<CapturedRecord>>> = Arc::new(Mutex::new(Vec::new()));
        let records_clone = Arc::clone(&records);

        let client_id = format!("lulu-recorder-{}", generate_random_string(6));
        let mut opts = MqttOptions::new(client_id, "127.0.0.1", port);
        opts.set_keep_alive(Duration::from_secs(5));

        let (sub_client, event_loop) = AsyncClient::new(opts, 256);

        // Subscribe to all lulu log topics
        sub_client
            .subscribe("lulu/#", QoS::AtMostOnce)
            .await?;

        let sub_handle = tokio::spawn(subscriber_loop(event_loop, records_clone))
            .abort_handle();

        Ok((
            Self {
                records,
                file_path,
                sub_handle,
                sub_client,
            },
            port,
        ))
    }

    /// Stops the subscriber, reads any existing `.lulu` file, merges with the
    /// new records, and writes the result.
    pub(crate) async fn stop(self) -> anyhow::Result<()> {
        // Disconnect subscriber gracefully; ignore errors (broker may be gone).
        let _ = self.sub_client.disconnect().await;

        // Give a short grace period so in-flight messages can be drained by
        // the subscriber loop before we abort it.
        tokio::time::sleep(Duration::from_millis(300)).await;

        self.sub_handle.abort();

        let new_records = self.records.lock().unwrap().clone();

        write_lulu_file(&self.file_path, &new_records)?;

        tracing::info!(
            "recorder: wrote {} record(s) to {}",
            new_records.len(),
            self.file_path.display()
        );

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Broker helpers
// ---------------------------------------------------------------------------

/// Binds to port 0 and returns the OS-assigned free port number.
fn free_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
    // `listener` is dropped here, releasing the port.
}

/// Spawns a dedicated thread that runs the embedded MQTT broker.
fn start_broker_thread(port: u16) {
    std::thread::spawn(move || {
        use rumqttd::{Broker, Config, ConnectionSettings, ServerSettings};

        let listen_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let mut servers = HashMap::new();
        servers.insert(
            "v4-1".to_string(),
            ServerSettings {
                name: "v4-1".to_string(),
                listen: std::net::SocketAddr::V4(listen_addr),
                tls: None,
                next_connection_delay_ms: 1,
                connections: ConnectionSettings {
                    connection_timeout_ms: 5_000,
                    max_payload_size: 102_400,
                    max_inflight_count: 200,
                    auth: None,
                    dynamic_filters: true,
                    external_auth: None,
                },
            },
        );

        let broker_config = Config {
            id: 0,
            router: rumqttd::RouterConfig {
                max_connections: 10,
                max_segment_size: 102_400,
                max_outgoing_packet_count: 200,
                max_segment_count: 10,
                ..Default::default()
            },
            v4: Some(servers),
            v5: None,
            ws: None,
            cluster: None,
            console: None,
            bridge: None,
            prometheus: None,
            metrics: None,
        };

        let mut broker = Broker::new(broker_config);
        tracing::info!("recorder: embedded broker listening on 127.0.0.1:{}", port);
        if let Err(e) = broker.start() {
            tracing::warn!("recorder: broker exited: {}", e);
        }
    });
}

// ---------------------------------------------------------------------------
// Subscriber loop
// ---------------------------------------------------------------------------

async fn subscriber_loop(
    mut event_loop: EventLoop,
    records: Arc<Mutex<Vec<CapturedRecord>>>,
) {
    loop {
        match event_loop.poll().await {
            Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish))) => {
                let topic   = publish.topic.clone();
                let payload = publish.payload.to_vec();
                records.lock().unwrap().push(CapturedRecord { topic, payload });
            }
            Ok(_) => {}
            Err(e) => {
                tracing::debug!("recorder subscriber: {}", e);
                // Retry after a short delay — broker may not be ready yet.
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

/// Reads the existing `.lulu` file (if present), merges with `new_records`,
/// and writes the combined result back to `path`.
fn write_lulu_file(path: &Path, new_records: &[CapturedRecord]) -> anyhow::Result<()> {
    // ── Load existing records ────────────────────────────────────────────────
    let mut existing: Vec<CapturedRecord> = Vec::new();
    if path.exists() {
        let bytes = std::fs::read(path)?;
        match root_as_lulu_export_file(&bytes) {
            Ok(export) => {
                for rec in export.records().iter() {
                    existing.push(CapturedRecord {
                        topic:   rec.topic().to_string(),
                        payload: rec.payload().iter().collect(),
                    });
                }
            }
            Err(e) => {
                tracing::warn!(
                    "recorder: existing file {} is not a valid .lulu file ({}), overwriting",
                    path.display(),
                    e
                );
            }
        }
    }

    // ── Merge ────────────────────────────────────────────────────────────────
    existing.extend_from_slice(new_records);

    // ── Serialise ────────────────────────────────────────────────────────────
    let bytes = serialise_lulu_export(&existing)?;

    // ── Write ────────────────────────────────────────────────────────────────
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, bytes)?;

    Ok(())
}

/// Serialises a list of records into a `.lulu` FlatBuffers buffer.
fn serialise_lulu_export(records: &[CapturedRecord]) -> anyhow::Result<Vec<u8>> {
    let mut builder = FlatBufferBuilder::with_capacity(records.len() * 256 + 1024);

    let fbs_records: Vec<_> = records
        .iter()
        .map(|r| {
            let topic   = builder.create_string(&r.topic);
            let payload = builder.create_vector(&r.payload);
            LogRecord::create(
                &mut builder,
                &LogRecordArgs {
                    topic:   Some(topic),
                    payload: Some(payload),
                },
            )
        })
        .collect();

    let records_vec = builder.create_vector(&fbs_records);
    let export = LuluExportFile::create(
        &mut builder,
        &LuluExportFileArgs {
            version: 1,
            records: Some(records_vec),
        },
    );
    builder.finish(export, None);

    Ok(builder.finished_data().to_vec())
}

// ---------------------------------------------------------------------------
// Default file path helper
// ---------------------------------------------------------------------------

/// Returns the default recording file path: `lulu_recording.lulu` in the
/// current working directory.
pub(crate) fn default_recording_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("lulu_recording.lulu")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_free_port_returns_nonzero() {
        let port = free_port().expect("should find a free port");
        assert!(port > 0);
    }

    #[test]
    fn test_serialise_and_parse_roundtrip() {
        let records = vec![
            CapturedRecord {
                topic:   "lulu/sensor/temperature".to_string(),
                payload: vec![1, 2, 3, 4],
            },
            CapturedRecord {
                topic:   "lulu/sensor/humidity".to_string(),
                payload: vec![5, 6, 7, 8],
            },
        ];

        let bytes = serialise_lulu_export(&records).unwrap();
        let parsed = root_as_lulu_export_file(&bytes).unwrap();

        assert_eq!(parsed.version(), 1);
        assert_eq!(parsed.records().len(), 2);
        assert_eq!(parsed.records().get(0).topic(), "lulu/sensor/temperature");
        assert_eq!(parsed.records().get(0).payload().iter().collect::<Vec<u8>>(), &[1u8, 2, 3, 4]);
        assert_eq!(parsed.records().get(1).topic(), "lulu/sensor/humidity");
        assert_eq!(parsed.records().get(1).payload().iter().collect::<Vec<u8>>(), &[5u8, 6, 7, 8]);
    }

    #[test]
    fn test_write_lulu_file_creates_new() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lulu");

        let records = vec![CapturedRecord {
            topic:   "lulu/test/attr".to_string(),
            payload: vec![0xDE, 0xAD],
        }];

        write_lulu_file(&path, &records).unwrap();
        assert!(path.exists());

        let bytes = std::fs::read(&path).unwrap();
        let parsed = root_as_lulu_export_file(&bytes).unwrap();
        assert_eq!(parsed.records().len(), 1);
        assert_eq!(parsed.records().get(0).topic(), "lulu/test/attr");
    }

    #[test]
    fn test_write_lulu_file_appends_to_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lulu");

        // First write
        let first = vec![CapturedRecord {
            topic:   "lulu/test/first".to_string(),
            payload: vec![1],
        }];
        write_lulu_file(&path, &first).unwrap();

        // Second write — should append
        let second = vec![CapturedRecord {
            topic:   "lulu/test/second".to_string(),
            payload: vec![2],
        }];
        write_lulu_file(&path, &second).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let parsed = root_as_lulu_export_file(&bytes).unwrap();
        assert_eq!(parsed.records().len(), 2);
        assert_eq!(parsed.records().get(0).topic(), "lulu/test/first");
        assert_eq!(parsed.records().get(1).topic(), "lulu/test/second");
    }

    #[test]
    fn test_default_recording_path_has_lulu_extension() {
        let path = default_recording_path();
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("lulu"));
    }
}
