//! # span — Generic span lifecycle helpers
//!
//! This module provides a fluent builder ([`SpanBuilder`]) and a RAII handle
//! ([`SpanHandle`]) for tracking arbitrary named operations as `span_beg` /
//! `span_end` log entries.
//!
//! Spans are more general than scenarios or steps: they require an explicit
//! source and attribute, and support custom `kind`, metadata and result
//! payloads.
//!
//! ## Lifecycle
//!
//! ```text
//! lulu_span("5V-calibration")
//!     .source("psu/channel-1")
//!     .attribute("calibration")
//!     .kind("calibration")
//!     .metadata(&json!({"reference_v": 5.0}))
//!     .terminal(true)
//!     .begin()                →  publishes  span_beg  →  SpanHandle
//!                ↓
//!          span.set_result(&json!({"avg_v": 4.997}))
//!          span.set_duration_ms(320)
//!          span.end()         →  publishes  span_end (success)
//!       or span.fail("msg")   →  publishes  span_end (failure)
//!       or  drop              →  publishes  span_end (success, auto)
//! ```

use serde_json::Value;

use crate::error::LuluError;
use crate::models::{Data, LogLevel};
use crate::{build_span_payload, lulu_publish, rand_util, terminal_logger, topic};

// ---------------------------------------------------------------------------
// SpanBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for a generic span.
///
/// Created by [`lulu_span`]. Configure with chained methods, then call
/// [`.begin()`](SpanBuilder::begin) to publish the `span_beg` entry and
/// obtain a [`SpanHandle`].
///
/// Both `source` and `attribute` are **required** — calling `.begin()` without
/// them returns a [`LuluError`].
pub struct SpanBuilder {
    /// Human-readable name of the span operation.
    name: String,
    /// MQTT source path — required before `.begin()`.
    source: Option<String>,
    /// MQTT attribute — required before `.begin()`.
    attribute: Option<String>,
    /// Span kind tag (e.g. `"calibration"`, `"measurement"`). Defaults to `"span"`.
    kind: Option<String>,
    /// Optional metadata included in the `span_beg` payload.
    metadata: Option<Value>,
    /// Whether to print terminal output for this span. Default: `false`.
    terminal: bool,
}

impl SpanBuilder {
    /// Sets the MQTT source path (e.g. `"psu/channel-1"`). **Required.**
    pub fn source(mut self, source: &str) -> Self {
        self.source = Some(source.to_string());
        self
    }

    /// Sets the MQTT attribute (e.g. `"calibration"`). **Required.**
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

    /// Enables or disables terminal output for this span. Default: `false`.
    pub fn terminal(mut self, enabled: bool) -> Self {
        self.terminal = enabled;
        self
    }

    /// Validates configuration, publishes the `span_beg` entry and returns a [`SpanHandle`].
    ///
    /// # Errors
    /// * [`LuluError::InvalidSource`] — `source` was not set or contains invalid segments.
    /// * [`LuluError::InvalidAttribute`] — `attribute` was not set or contains invalid characters.
    /// * [`LuluError::NotInitialized`] — [`lulu_init`](crate::lulu_init) was not called.
    /// * [`LuluError::QueueFull`] — the publish queue is saturated.
    pub fn begin(self) -> Result<SpanHandle, LuluError> {
        let source = self
            .source
            .as_deref()
            .ok_or_else(|| LuluError::InvalidSource("source is required".to_string()))?;
        let attribute = self
            .attribute
            .as_deref()
            .ok_or_else(|| LuluError::InvalidAttribute("attribute is required".to_string()))?;

        // Validate source and attribute before publishing.
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

// ---------------------------------------------------------------------------
// SpanHandle
// ---------------------------------------------------------------------------

/// Handle returned by [`SpanBuilder::begin`] to control the end of a generic span.
///
/// Optionally enrich the `span_end` entry with metadata, result and duration
/// via the `set_*` methods, then call [`.end()`](SpanHandle::end) (success)
/// or [`.fail()`](SpanHandle::fail) (failure).
///
/// If dropped without calling either, the span is automatically ended as a
/// success.
pub struct SpanHandle {
    /// MQTT source path.
    source: String,
    /// MQTT attribute.
    attribute: String,
    /// Unique span identifier for correlating `span_beg` / `span_end`.
    span_id: String,
    /// Human-readable span name.
    name: String,
    /// Span kind tag resolved at creation time.
    kind: String,
    /// Whether to print terminal output for this span.
    terminal: bool,
    /// Optional metadata for the `span_end` entry.
    metadata: Option<Value>,
    /// Optional result payload for the `span_end` entry.
    result: Option<Value>,
    /// Optional explicit duration (milliseconds). If `None`, no `duration_ms` field is emitted.
    duration_ms: Option<u64>,
    /// Set to `true` once `.end()` or `.fail()` is called.
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
    ///
    /// If not set, no `duration_ms` field is included in the payload.
    pub fn set_duration_ms(&mut self, duration_ms: u64) {
        self.duration_ms = Some(duration_ms);
    }

    /// Publishes a successful `span_end` log entry.
    ///
    /// # Errors
    /// Returns a [`LuluError`] if publishing fails (e.g. queue full).
    pub fn end(mut self) -> Result<(), LuluError> {
        self.finished = true;
        self.finish(true, None)
    }

    /// Publishes a failed `span_end` log entry with an error message.
    ///
    /// # Errors
    /// Returns a [`LuluError`] if publishing fails (e.g. queue full).
    pub fn fail(mut self, error: &str) -> Result<(), LuluError> {
        self.finished = true;
        self.finish(false, Some(error))
    }

    /// Internal publish — called by `.end()`, `.fail()` and `Drop`.
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
    /// Automatically ends the span as a success if `.end()` or `.fail()` was never called.
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(true, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Creates a [`SpanBuilder`] for a generic span with the given name.
///
/// Configure source, attribute, kind, metadata and terminal output via the
/// builder, then call [`.begin()`](SpanBuilder::begin) to publish the
/// `span_beg` entry and obtain a [`SpanHandle`].
///
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
