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
mod terminal_logger;
mod topic;

#[allow(dead_code, unused_imports, clippy::all)]
mod lulu_logs_generated;

#[allow(dead_code, unused_imports, clippy::all)]
mod lulu_export_generated;

// --- Public re-exports ---
pub use client::{LuluConfig, LuluStats};
pub use error::LuluError;
pub use models::{Data, DataType, LogLevel};
pub use publisher::LuluPublisher;

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

fn build_span_payload(
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

/// Handle returned by [`lulu_scenario`] to mark the end of a test scenario.
///
/// Call [`.end()`](ScenarioHandle::end) to publish the `scenario_end` entry.
/// If dropped without calling `.end()`, the scenario is automatically ended
/// as a success.
pub struct ScenarioHandle {
    scenario_name: String,
    finished: bool,
}

impl ScenarioHandle {
    /// Publishes a `step_beg` log entry and returns a [`StepHandle`].
    ///
    /// Source and attribute are inherited from the scenario (`"test"` / `"scenario"`).
    /// The `span_id` is auto-generated as `"step-{step_name}-{random6}"`.
    /// Prints a coloured `▸ step_name` line when `terminal_logger` is enabled.
    pub fn step(&self, step_name: &str) -> Result<StepHandle, LuluError> {
        self.step_with_metadata(step_name, None)
    }

    /// Same as [`step`](Self::step) but attaches metadata to the `step_beg` entry.
    pub fn step_with_metadata(
        &self,
        step_name: &str,
        metadata: Option<&Value>,
    ) -> Result<StepHandle, LuluError> {
        terminal_logger::print_step_beg(step_name);
        let span_id = format!(
            "step-{}-{}",
            step_name,
            rand_util::generate_random_string(6)
        );
        let json = build_span_payload(
            &span_id,
            Some(step_name),
            None,
            None,
            None,
            None,
            metadata,
            None,
        );
        lulu_publish("test", "scenario", LogLevel::Info, Data::StepBeg(json))?;
        Ok(StepHandle {
            source: "test".to_string(),
            attribute: "scenario".to_string(),
            span_id,
            step_name: step_name.to_string(),
            finished: false,
        })
    }

    /// Publishes a `scenario_end` log entry for this scenario.
    ///
    /// Prints a coloured `✓` / `✗` line when `terminal_logger` is enabled.
    pub fn end(mut self, success: bool, error: Option<&str>) -> Result<(), LuluError> {
        self.finished = true;
        self.finish(success, error)
    }

    fn finish(&self, success: bool, error: Option<&str>) -> Result<(), LuluError> {
        terminal_logger::print_end(&self.scenario_name, success, error);
        let span_id = format!("scenario-{}", self.scenario_name);
        let json = build_span_payload(
            &span_id,
            Some(&self.scenario_name),
            None,
            Some(success),
            error,
            None,
            None,
            None,
        );
        let level = if success {
            LogLevel::Info
        } else {
            LogLevel::Error
        };
        lulu_publish("test", "scenario", level, Data::ScenarioEnd(json))
    }
}

impl Drop for ScenarioHandle {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(true, None);
        }
    }
}

/// Publishes a `scenario_beg` log entry and returns a [`ScenarioHandle`].
///
/// Source is always `"test"`, attribute is always `"scenario"`.
/// The `span_id` is derived as `"scenario-{scenario_name}"`.
/// Prints a coloured `▶ scenario_name` line when `terminal_logger` is enabled.
pub fn lulu_scenario(scenario_name: &str) -> Result<ScenarioHandle, LuluError> {
    terminal_logger::print_beg(scenario_name);
    let span_id = format!("scenario-{}", scenario_name);
    let json = build_span_payload(
        &span_id,
        Some(scenario_name),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    lulu_publish("test", "scenario", LogLevel::Info, Data::ScenarioBeg(json))?;
    Ok(ScenarioHandle {
        scenario_name: scenario_name.to_string(),
        finished: false,
    })
}

/// Builder for a generic span.
///
/// Created by [`lulu_span`].  Configure with chained methods, then call
/// [`.begin()`](SpanBuilder::begin) to publish the `span_beg` entry and
/// obtain a [`SpanHandle`].
///
/// # Example
/// ```no_run
/// use lulu_logs_client::lulu_span;
/// use serde_json::json;
///
/// let mut span = lulu_span("5V-calibration")
///     .source("psu/channel-1")
///     .attribute("calibration")
///     .kind("calibration")
///     .metadata(&json!({"reference_v": 5.0}))
///     .terminal(true)
///     .begin()
///     .unwrap();
///
/// span.set_result(&json!({"avg_v": 4.997}));
/// span.set_duration_ms(320);
/// span.end().unwrap();
/// ```
pub struct SpanBuilder {
    name: String,
    source: Option<String>,
    attribute: Option<String>,
    kind: Option<String>,
    metadata: Option<Value>,
    terminal: bool,
}

impl SpanBuilder {
    /// Sets the MQTT source path (e.g. `"psu/channel-1"`).  **Required.**
    pub fn source(mut self, source: &str) -> Self {
        self.source = Some(source.to_string());
        self
    }

    /// Sets the MQTT attribute (e.g. `"calibration"`).  **Required.**
    pub fn attribute(mut self, attribute: &str) -> Self {
        self.attribute = Some(attribute.to_string());
        self
    }

    /// Sets the span kind (e.g. `"calibration"`, `"measurement"`).
    /// Defaults to `"span"` if not set.
    pub fn kind(mut self, kind: &str) -> Self {
        self.kind = Some(kind.to_string());
        self
    }

    /// Attaches metadata to the `span_beg` entry.
    pub fn metadata(mut self, metadata: &Value) -> Self {
        self.metadata = Some(metadata.clone());
        self
    }

    /// Enables or disables terminal output for this span.  Default: `false`.
    pub fn terminal(mut self, enabled: bool) -> Self {
        self.terminal = enabled;
        self
    }

    /// Validates configuration, publishes the `span_beg` entry and returns
    /// a [`SpanHandle`].
    ///
    /// # Errors
    /// Returns [`LuluError::InvalidSource`] if `source` was not set or is
    /// invalid, and [`LuluError::InvalidAttribute`] if `attribute` was not
    /// set or is invalid.
    pub fn begin(self) -> Result<SpanHandle, LuluError> {
        let source = self
            .source
            .as_deref()
            .ok_or(LuluError::InvalidSource("source is required".to_string()))?;
        let attribute = self
            .attribute
            .as_deref()
            .ok_or(LuluError::InvalidAttribute(
                "attribute is required".to_string(),
            ))?;

        topic::parse_source(source)?;
        topic::validate_attribute(attribute)?;

        let kind = self.kind.as_deref().unwrap_or("span");
        let span_id = format!(
            "span-{}-{}",
            self.name,
            rand_util::generate_random_string(6)
        );

        let json = build_span_payload(
            &span_id,
            Some(&self.name),
            Some(kind),
            None,
            None,
            None,
            self.metadata.as_ref(),
            None,
        );

        if self.terminal {
            terminal_logger::print_span_beg(&self.name);
        }

        lulu_publish(source, attribute, LogLevel::Info, Data::SpanBeg(json))?;

        Ok(SpanHandle {
            source: source.to_string(),
            attribute: attribute.to_string(),
            span_id,
            name: self.name,
            kind: kind.to_string(),
            terminal: self.terminal,
            metadata: None,
            result: None,
            duration_ms: None,
            finished: false,
        })
    }
}

/// Handle returned by [`SpanBuilder::begin`] to mark the end of a generic span.
///
/// Set optional metadata, result and duration via the `set_*` methods, then
/// call [`.end()`](SpanHandle::end) (success) or
/// [`.fail()`](SpanHandle::fail) (failure) to publish the `span_end` entry.
/// If dropped without calling either, the span is automatically ended as a
/// success.
pub struct SpanHandle {
    source: String,
    attribute: String,
    span_id: String,
    name: String,
    kind: String,
    terminal: bool,
    metadata: Option<Value>,
    result: Option<Value>,
    duration_ms: Option<u64>,
    finished: bool,
}

impl SpanHandle {
    /// Attaches or replaces metadata for the `span_end` entry.
    pub fn set_metadata(&mut self, metadata: &Value) {
        self.metadata = Some(metadata.clone());
    }

    /// Attaches or replaces the result payload for the `span_end` entry.
    pub fn set_result(&mut self, result: &Value) {
        self.result = Some(result.clone());
    }

    /// Sets the span duration (milliseconds) for the `span_end` entry.
    pub fn set_duration_ms(&mut self, duration_ms: u64) {
        self.duration_ms = Some(duration_ms);
    }

    /// Publishes a successful `span_end` log entry.
    pub fn end(mut self) -> Result<(), LuluError> {
        self.finished = true;
        self.finish(true, None)
    }

    /// Publishes a failed `span_end` log entry with an error message.
    pub fn fail(mut self, error: &str) -> Result<(), LuluError> {
        self.finished = true;
        self.finish(false, Some(error))
    }

    fn finish(&self, success: bool, error: Option<&str>) -> Result<(), LuluError> {
        if self.terminal {
            terminal_logger::print_span_end(&self.name, success, error);
        }

        let json = build_span_payload(
            &self.span_id,
            Some(&self.name),
            Some(&self.kind),
            Some(success),
            error,
            self.duration_ms,
            self.metadata.as_ref(),
            self.result.as_ref(),
        );

        let level = if success {
            LogLevel::Info
        } else {
            LogLevel::Error
        };
        lulu_publish(&self.source, &self.attribute, level, Data::SpanEnd(json))
    }
}

impl Drop for SpanHandle {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(true, None);
        }
    }
}

/// Creates a [`SpanBuilder`] for a generic span with the given name.
///
/// Configure source, attribute, kind, metadata and terminal via the builder,
/// then call [`.begin()`](SpanBuilder::begin) to publish the `span_beg`
/// entry and obtain a [`SpanHandle`].
pub fn lulu_span(name: &str) -> SpanBuilder {
    SpanBuilder {
        name: name.to_string(),
        source: None,
        attribute: None,
        kind: None,
        metadata: None,
        terminal: false,
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

/// Handle returned by [`lulu_step`] to mark the end of a step.
///
/// Call [`.end()`](StepHandle::end) to publish the `step_end` entry.
/// If dropped without calling `.end()`, the step is automatically ended
/// as a success.
pub struct StepHandle {
    source: String,
    attribute: String,
    span_id: String,
    step_name: String,
    finished: bool,
}

impl StepHandle {
    /// Publishes a `step_end` log entry for this step.
    ///
    /// Prints a coloured `✓` / `✗` line when `terminal_logger` is enabled.
    pub fn end(
        mut self,
        success: bool,
        error: Option<&str>,
        duration_ms: Option<u64>,
        metadata: Option<&Value>,
        result: Option<&Value>,
    ) -> Result<(), LuluError> {
        self.finished = true;
        self.finish(success, error, duration_ms, metadata, result)
    }

    fn finish(
        &self,
        success: bool,
        error: Option<&str>,
        duration_ms: Option<u64>,
        metadata: Option<&Value>,
        result: Option<&Value>,
    ) -> Result<(), LuluError> {
        terminal_logger::print_step_end(&self.step_name, success, error);
        let json = build_span_payload(
            &self.span_id,
            Some(&self.step_name),
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
        lulu_publish(&self.source, &self.attribute, level, Data::StepEnd(json))
    }
}

impl Drop for StepHandle {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(true, None, None, None, None);
        }
    }
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
