//! # scenario — Test scenario lifecycle helpers
//!
//! This module provides [`ScenarioHandle`] and [`lulu_scenario`], the
//! top-level entry point for structured test execution tracking.
//!
//! ## Lifecycle
//!
//! ```text
//! lulu_scenario("my-test")   →  publishes  scenario_beg
//!         ↓
//!   ScenarioHandle
//!         ├── .step("step-1")             →  see step module
//!         ├── .step_with_metadata(...)    →  see step module
//!         └── .end(Ok(()))               →  publishes  scenario_end (success)
//!          or .end(Err(e))               →  publishes  scenario_end (failure)
//!          or  drop (without .end)       →  publishes  scenario_end (success, auto)
//! ```
//!
//! ## Terminal output (when `terminal_logger` is enabled)
//!
//! * `▶ scenario-name` — scenario started
//! * `✓ scenario-name` — scenario passed
//! * `✗ scenario-name — error …` — scenario failed
//! * (step output is delegated to the [`step`](crate::step) module)

use serde_json::Value;

use crate::models::{Data, LogLevel};
use crate::step::StepHandle;
use crate::{
    build_span_payload, handle_scenario_publish_error, lulu_publish, rand_util, terminal_logger,
};

// ---------------------------------------------------------------------------
// ScenarioHandle
// ---------------------------------------------------------------------------

/// Handle returned by [`lulu_scenario`] to control the lifecycle of a test scenario.
///
/// Start steps with [`.step()`](ScenarioHandle::step) or
/// [`.step_with_metadata()`](ScenarioHandle::step_with_metadata), then call
/// [`.end()`](ScenarioHandle::end) to close the scenario.
///
/// If the handle is dropped without calling `.end()`, the scenario is
/// automatically ended as a success.
pub struct ScenarioHandle {
    /// The name passed to [`lulu_scenario`] — used as MQTT source and in terminal output.
    scenario_name: String,
    /// Set to `true` once `.end()` is called to prevent a second publish on drop.
    finished: bool,
}

impl ScenarioHandle {
    /// Publishes a `step_beg` log entry and returns a [`StepHandle`].
    ///
    /// This is a convenience wrapper around
    /// [`step_with_metadata`](Self::step_with_metadata) with no metadata.
    ///
    /// * **Source** — `"{scenario_name}/{step_name}"`
    /// * **Attribute** — `"step"`
    /// * **`span_id`** — `"step-{step_name}-{random6}"`
    ///
    /// Prints a coloured `▸ step_name` line when `terminal_logger` is enabled.
    ///
    /// # Panics
    /// Panics if the client is not initialised. A full publish queue is
    /// treated as a warning and silently ignored.
    pub fn step(&self, step_name: &str) -> StepHandle {
        self.step_with_metadata(step_name, None)
    }

    /// Publishes a `step_beg` log entry with optional metadata and returns a [`StepHandle`].
    ///
    /// * **Source** — `"{scenario_name}/{step_name}"`
    /// * **Attribute** — `"step"`
    /// * **`span_id`** — `"step-{step_name}-{random6}"`
    ///
    /// Prints a coloured `▸ step_name` line when `terminal_logger` is enabled.
    ///
    /// # Panics
    /// Panics if the client is not initialised. A full publish queue is
    /// treated as a warning and silently ignored.
    pub fn step_with_metadata(&self, step_name: &str, metadata: Option<&Value>) -> StepHandle {
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

        let source = format!("{}/{}", self.scenario_name, step_name);
        let attribute = "step";

        if let Err(e) = lulu_publish(&source, attribute, LogLevel::Info, Data::StepBeg(json)) {
            handle_scenario_publish_error(e);
        }

        StepHandle {
            source,
            attribute: attribute.to_string(),
            span_id,
            step_name: step_name.to_string(),
            metadata: metadata.cloned(),
            start_time: std::time::Instant::now(),
            finished: false,
        }
    }

    /// Publishes a `scenario_end` log entry and closes the scenario.
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
        if let Err(e) = lulu_publish(
            &self.scenario_name,
            "scenario",
            level,
            Data::ScenarioEnd(json),
        ) {
            handle_scenario_publish_error(e);
        }
    }
}

impl Drop for ScenarioHandle {
    /// Automatically ends the scenario as a success if `.end()` was never called.
    fn drop(&mut self) {
        if !self.finished {
            self.finish(true, None);
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Publishes a `scenario_beg` log entry and returns a [`ScenarioHandle`].
///
/// * **Source** — `"{scenario_name}"`
/// * **Attribute** — `"scenario"`
/// * **`span_id`** — `"scenario-{scenario_name}"`
///
/// Prints a coloured `▶ scenario_name` line when `terminal_logger` is enabled.
///
/// # Panics
/// Panics if the client is not initialised. A full publish queue is
/// treated as a warning and silently ignored.
///
pub fn lulu_scenario(scenario_name: &str) -> ScenarioHandle {
    terminal_logger::print_beg(scenario_name);

    let span_id = format!("scenario-{}", scenario_name);
    let json = build_span_payload(&span_id, Some(scenario_name), None, None, None, None, None, None);

    if let Err(e) = lulu_publish(
        scenario_name,
        "scenario",
        LogLevel::Info,
        Data::ScenarioBeg(json),
    ) {
        handle_scenario_publish_error(e);
    }

    ScenarioHandle {
        scenario_name: scenario_name.to_string(),
        finished: false,
    }
}
