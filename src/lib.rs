//! # lulu-logs-client
//!
//! MQTT client library for the **lulu-logs** protocol.
//!
//! This crate provides a singleton API (`lulu_init`, `lulu_publish`, `lulu_shutdown`)
//! that serialises log entries as FlatBuffers payloads and publishes them over MQTT.

use std::future::Future;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

mod client;
mod error;
mod models;
mod publisher;
mod rand_util;
mod recorder;
mod serializer;
mod source;
mod terminal_logger;
mod topic;

// Domain modules
pub mod scenario;
pub mod span;
pub mod step;

#[allow(dead_code, unused_imports, clippy::all)]
mod lulu_logs_generated;

#[allow(dead_code, unused_imports, clippy::all)]
mod lulu_export_generated;

// --- Public re-exports ---
pub use client::{LuluConfig, LuluConfigBuilder, LuluStats};
pub use error::LuluError;
pub use models::{Data, DataType, LogLevel};
pub use publisher::LuluPublisher;
pub use source::LuluSource;
pub use scenario::{lulu_scenario, ScenarioHandle};
pub use span::{lulu_span, SpanBuilder, SpanHandle};
pub use step::StepHandle;

use client::LuluClient;
use recorder::LuluRecorder;
use serde_json::{Map, Value};
use serializer::PendingMessage;

// ---------------------------------------------------------------------------
// Singletons
// ---------------------------------------------------------------------------

static GLOBAL_CLIENT: OnceLock<LuluClient> = OnceLock::new();
static TOKIO_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static GLOBAL_RECORDER: OnceLock<Mutex<Option<LuluRecorder>>> = OnceLock::new();

/// Returns (or lazily creates) the dedicated single-threaded tokio runtime.
pub(crate) fn get_or_init_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("lulu-logs-client-rt")
            .enable_all()
            .build()
            .expect("failed to create lulu-logs-client tokio runtime")
    })
}

/// Drives `fut` to completion, working whether or not the calling thread is
/// already inside a Tokio runtime.
///
/// * **Inside a multi-thread runtime** (`#[tokio::main]`, test harness, …):
///   uses [`tokio::task::block_in_place`] so we can block without holding the
///   runtime scheduler.
/// * **No runtime on this thread**: falls back to the crate-local dedicated
///   runtime (`TOKIO_RUNTIME`).
///
/// # Panics
/// Panics if called from inside a *current-thread* (`basic_scheduler`) runtime,
/// because `block_in_place` requires a multi-thread runtime.
fn block_on_smart<F>(fut: F) -> F::Output
where
    F: Future,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => get_or_init_runtime().block_on(fut),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialises the global lulu-logs-client singleton.
///
/// Must be called exactly once before any `lulu_publish()`. Calling it a second
/// time returns `Err(LuluError::AlreadyInitialized)`.
pub fn lulu_init(config: LuluConfig) -> Result<(), LuluError> {
    if GLOBAL_CLIENT.get().is_some() {
        return Err(LuluError::AlreadyInitialized);
    }

    // Activate the optional terminal logger for test scenarios.
    terminal_logger::set_enabled(config.terminal_logger);

    let client =
        block_on_smart(LuluClient::start(config)).map_err(|_| LuluError::AlreadyInitialized)?;

    GLOBAL_CLIENT
        .set(client)
        .map_err(|_| LuluError::AlreadyInitialized)
}

/// Publishes a log entry onto the MQTT bus.
///
/// The message is validated, enqueued, then serialised and published
/// asynchronously by the background send-loop.
pub fn lulu_publish(
    source: &str,
    attribute: &str,
    level: LogLevel,
    data: Data,
) -> Result<(), LuluError> {
    let client = GLOBAL_CLIENT.get().ok_or(LuluError::NotInitialized)?;

    let source_segments = topic::parse_source(source)?;
    topic::validate_attribute(attribute)?;

    client.publish(PendingMessage {
        source_segments,
        attribute: attribute.to_string(),
        level,
        data,
    })
}

/// Gracefully shuts down the lulu-logs-client.
///
/// Waits up to 5 seconds for the internal queue to drain before returning.
/// If the client was never initialised, this function returns immediately.
pub fn lulu_shutdown() {
    let client = match GLOBAL_CLIENT.get() {
        Some(c) => c,
        None => return,
    };

    // Stop all heartbeat tasks before draining the log queue.
    client.stop_all_pulses();

    let _ = block_on_smart(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if client.stats().queue_current_size == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
    });

    // OnceLock cannot be reset — the connection will close when the process exits.
    // If init → shutdown → init cycles are required, replace OnceLock<LuluClient>
    // with Mutex<Option<LuluClient>>.
}

/// Returns `true` if `lulu_init()` has been called successfully.
pub fn lulu_is_initialized() -> bool {
    GLOBAL_CLIENT.get().is_some()
}

/// Returns `true` if the MQTT connection is currently alive.
pub fn lulu_is_connected() -> bool {
    GLOBAL_CLIENT
        .get()
        .map(|c| c.is_connected())
        .unwrap_or(false)
}

/// Returns runtime statistics, or `None` if the client is not initialised.
pub fn lulu_stats() -> Option<LuluStats> {
    GLOBAL_CLIENT.get().map(|c| c.stats())
}

// ---------------------------------------------------------------------------
// Heartbeat helpers (lulu-logs v1.2.0 §7)
// ---------------------------------------------------------------------------

/// Starts a background heartbeat on `lulu-pulse/{source}` every 2 seconds.
///
/// The first pulse is emitted immediately upon registration. Calling again
/// with the same source replaces the existing task (idempotent).
///
/// # Errors
/// Returns `LuluError::NotInitialized` if `lulu_init()` has not been called,
/// or `LuluError::InvalidSource` if `source` contains invalid segments.
pub fn lulu_start_pulse(source: &str, version: Option<&str>) -> Result<(), LuluError> {
    let client = GLOBAL_CLIENT.get().ok_or(LuluError::NotInitialized)?;
    let source_segments = topic::parse_source(source)?;
    let pulse_topic = topic::build_pulse_topic(&source_segments);
    let rt = get_or_init_runtime();
    client.start_pulse(
        source.to_string(),
        pulse_topic,
        version.map(str::to_string),
        rt,
    );
    Ok(())
}

/// Stops the heartbeat for the given source. No-op if no pulse is running
/// for that source, or if the client is not initialised.
pub fn lulu_stop_pulse(source: &str) {
    if let Some(client) = GLOBAL_CLIENT.get() {
        client.stop_pulse(source);
    }
}

// ---------------------------------------------------------------------------
// Span convenience helpers (lulu-logs v1.3.0 §3.4)
// ---------------------------------------------------------------------------

pub(crate) fn build_span_payload(
    span_id: &str,
    name: Option<&str>,
    kind: Option<&str>,
    success: Option<bool>,
    error: Option<&str>,
    duration_ms: Option<u64>,
    metadata: Option<&Value>,
    result: Option<&Value>,
) -> String {
    let mut payload = Map::new();
    payload.insert("span_id".to_string(), Value::String(span_id.to_string()));

    if let Some(name) = name {
        payload.insert("name".to_string(), Value::String(name.to_string()));
    }

    if let Some(kind) = kind {
        payload.insert("kind".to_string(), Value::String(kind.to_string()));
    }

    if let Some(success) = success {
        payload.insert("success".to_string(), Value::Bool(success));
    }

    if let Some(error) = error {
        payload.insert("error".to_string(), Value::String(error.to_string()));
    }

    if let Some(duration_ms) = duration_ms {
        payload.insert("duration_ms".to_string(), Value::from(duration_ms));
    }

    if let Some(metadata) = metadata {
        payload.insert("metadata".to_string(), metadata.clone());
    }

    if let Some(result) = result {
        payload.insert("result".to_string(), result.clone());
    }

    Value::Object(payload).to_string()
}

/// Handles errors from `lulu_publish` in the scenario/step context.
///
/// * `QueueFull` is treated as a non-blocking warning and is logged via
///   `eprintln!` before being ignored.
/// * Any other error (e.g. `NotInitialized`) is a blocking programming error
///   and causes a panic.
pub(crate) fn handle_scenario_publish_error(err: LuluError) {
    match err {
        LuluError::QueueFull => {
            eprintln!("[lulu-logs] warning: publish queue is full, message dropped");
        }
        other => {
            panic!("[lulu-logs] fatal scenario error: {other}");
        }
    }
}

/// Publishes a specialized `tool_call_beg` log entry.
pub fn lulu_tool_call_beg(
    source: &str,
    attribute: &str,
    span_id: &str,
    tool_name: &str,
    metadata: Option<&Value>,
) -> Result<(), LuluError> {
    let json = build_span_payload(
        span_id,
        Some(tool_name),
        None,
        None,
        None,
        None,
        metadata,
        None,
    );
    lulu_publish(source, attribute, LogLevel::Info, Data::ToolCallBeg(json))
}

/// Publishes a specialized `tool_call_end` log entry.
pub fn lulu_tool_call_end(
    source: &str,
    attribute: &str,
    span_id: &str,
    tool_name: &str,
    success: bool,
    error: Option<&str>,
    duration_ms: Option<u64>,
    metadata: Option<&Value>,
    result: Option<&Value>,
) -> Result<(), LuluError> {
    let json = build_span_payload(
        span_id,
        Some(tool_name),
        None,
        Some(success),
        error,
        duration_ms,
        metadata,
        result,
    );
    let level = if success {
        LogLevel::Info
    } else {
        LogLevel::Error
    };
    lulu_publish(source, attribute, level, Data::ToolCallEnd(json))
}

// ---------------------------------------------------------------------------
// Embedded recorder (lulu-logs v1.3.0)
// ---------------------------------------------------------------------------

fn get_or_init_recorder() -> &'static Mutex<Option<LuluRecorder>> {
    GLOBAL_RECORDER.get_or_init(|| Mutex::new(None))
}

/// Starts an embedded MQTT broker and records all `lulu/#` log entries to a
/// `.lulu` file.
///
/// This function also calls [`lulu_init`] internally so the current process
/// can publish logs immediately after calling `lulu_start_recorder`.
/// If `lulu_init` was already called this function returns
/// `Err(LuluError::AlreadyInitialized)`.
///
/// # Arguments
/// * `file_path` — destination `.lulu` file.  Pass `None` to use the default
///   path (`lulu_recording.lulu` in the current working directory).  If the
///   file already exists its records are preserved and the new entries are
///   appended on [`lulu_stop_recorder`].
///
/// # Example
/// ```no_run
/// use lulu_logs_client::{lulu_start_recorder, lulu_stop_recorder, lulu_publish, LogLevel, Data};
///
/// lulu_start_recorder(None).unwrap();
/// lulu_publish("my-service", "status", LogLevel::Info, Data::String("ok".into())).unwrap();
/// lulu_stop_recorder().unwrap();
/// ```
pub fn lulu_start_recorder(file_path: Option<PathBuf>) -> Result<(), LuluError> {
    let path = file_path.unwrap_or_else(recorder::default_recording_path);

    // Start the broker and subscriber, and get the broker port.
    let (rec, port) = block_on_smart(recorder::LuluRecorder::start(path)).map_err(|e| {
        tracing::error!("recorder: failed to start: {}", e);
        LuluError::RecorderStartFailed
    })?;

    // Store the recorder singleton.
    {
        let mut guard = get_or_init_recorder().lock().unwrap();
        if guard.is_some() {
            return Err(LuluError::AlreadyInitialized);
        }
        *guard = Some(rec);
    }

    // Initialise the publish client pointing at the embedded broker.
    lulu_init(LuluConfig {
        broker_host: "127.0.0.1".to_string(),
        broker_port: port,
        ..Default::default()
    })
}

/// Stops the embedded recorder, waits for in-flight messages to be captured,
/// then writes (or appends to) the `.lulu` file.
///
/// Also calls [`lulu_shutdown`] to drain the publish queue before writing.
/// If the recorder was never started this function is a no-op.
pub fn lulu_stop_recorder() -> Result<(), LuluError> {
    // Drain the publish queue first.
    lulu_shutdown();

    let rec = {
        let mut guard = get_or_init_recorder().lock().unwrap();
        guard.take()
    };

    if let Some(rec) = rec {
        block_on_smart(rec.stop()).map_err(|e| {
            tracing::error!("recorder: failed to stop or write file: {}", e);
            LuluError::RecorderStopFailed
        })?;
    }

    Ok(())
}
