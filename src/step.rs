//! # step — Test step lifecycle helpers
//!
//! This module provides [`StepHandle`], the RAII handle returned when a test
//! step is started through [`ScenarioHandle::step`](crate::scenario::ScenarioHandle::step).
//!
//! ## Lifecycle
//!
//! ```text
//! ScenarioHandle::step("my-step")  →  publishes  step_beg
//!                     ↓
//!              StepHandle
//!                     ↓
//!           .end(Ok(()))           →  publishes  step_end (success)
//!        or .end(Err(e))           →  publishes  step_end (failure, with error)
//!        or  drop (without .end)   →  publishes  step_end (success, auto)
//! ```
//!
//! Duration is measured automatically from the moment the handle is created.
//!
//! Terminal output (when `terminal_logger` is enabled):
//! * `  ▸ step-name` on start
//! * `  ✓ step-name` on success
//! * `  ✗ step-name — error …` on failure

use std::time::Instant;

use serde_json::Value;

use crate::models::{Data, LogLevel};
use crate::{build_span_payload, handle_scenario_publish_error, lulu_publish, terminal_logger};

// ---------------------------------------------------------------------------
// StepHandle
// ---------------------------------------------------------------------------

/// Handle returned by [`ScenarioHandle::step`](crate::scenario::ScenarioHandle::step)
/// to mark the end of a test step.
///
/// Call [`.end()`](StepHandle::end) to publish the `step_end` entry with an
/// explicit success/failure result. If the handle is dropped without calling
/// `.end()`, the step is automatically ended as a success.
///
/// The duration reported in `step_end` is measured from the instant the
/// handle is created.
pub struct StepHandle {
    /// MQTT source path (e.g. `"scenario-name/step-name"`).
    pub(crate) source: String,
    /// MQTT attribute (always `"step"`).
    pub(crate) attribute: String,
    /// Unique span identifier for correlating `step_beg` / `step_end`.
    pub(crate) span_id: String,
    /// Human-readable step name.
    pub(crate) step_name: String,
    /// Optional metadata attached to the `step_end` entry.
    pub(crate) metadata: Option<Value>,
    /// Instant the step was created — used to compute duration.
    pub(crate) start_time: Instant,
    /// Set to `true` once `.end()` is called to prevent a second publish on drop.
    pub(crate) finished: bool,
}

impl StepHandle {
    /// Attaches or replaces metadata for the `step_end` entry.
    ///
    /// Can be called at any point before `.end()`.
    pub fn set_metadata(&mut self, metadata: &Value) {
        self.metadata = Some(metadata.clone());
    }

    /// Publishes a `step_end` log entry for this step.
    ///
    /// The duration is automatically calculated from the time the step was
    /// created. Metadata stored via [`set_metadata`](Self::set_metadata) (or
    /// passed at creation time) is included in the entry.
    ///
    /// Prints a coloured `✓` / `✗` line when `terminal_logger` is enabled.
    ///
    /// # Panics
    /// Panics if the client is not initialised. A full publish queue is
    /// treated as a warning and silently ignored.
    pub fn end(mut self, result: anyhow::Result<()>) {
        self.finished = true;
        match result {
            Ok(()) => self.finish(true, None),
            Err(e) => {
                let err_str = e.to_string();
                self.finish(false, Some(&err_str));
            }
        }
    }

    /// Internal publish — called by both `.end()` and `Drop`.
    fn finish(&self, success: bool, error: Option<&str>) {
        let duration_ms = Some(self.start_time.elapsed().as_millis() as u64);
        terminal_logger::print_step_end(&self.step_name, success, error);
        let json = build_span_payload(
            &self.span_id,
            Some(&self.step_name),
            None,
            Some(success),
            error,
            duration_ms,
            self.metadata.as_ref(),
            None,
        );
        let level = if success {
            LogLevel::Info
        } else {
            LogLevel::Error
        };
        if let Err(e) = lulu_publish(&self.source, &self.attribute, level, Data::StepEnd(json)) {
            handle_scenario_publish_error(e);
        }
    }
}

impl Drop for StepHandle {
    /// Automatically ends the step as a success if `.end()` was never called.
    fn drop(&mut self) {
        if !self.finished {
            self.finish(true, None);
        }
    }
}
