use crate::error::LuluError;
use crate::models::{Data, LogLevel};
use crate::serializer::PendingMessage;
use crate::terminal_logger;
use crate::topic;
use crate::GLOBAL_CLIENT;

/// A pre-validated publisher bound to a fixed `source` and `attribute`.
///
/// Created via [`LuluPublisher::new`], which validates the source and attribute
/// once at construction time.  The convenience methods (`debug`, `info`, `warn`,
/// `error`, …) only require a [`Data`] payload, avoiding repeated validation on
/// every publish call.
///
/// When `terminal` is `true`, every publish also prints a coloured one-liner to
/// stdout so a developer can follow the data flow at a glance.
///
/// # Example
/// ```no_run
/// use lulu_logs_client::{LuluPublisher, Data};
///
/// let voltage = LuluPublisher::new("psu/channel-1", "voltage")
///     .unwrap()
///     .terminal(true);
/// voltage.info(Data::Float32(3.31)).unwrap();
/// voltage.warn(Data::Float32(2.80)).unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct LuluPublisher {
    source_segments: Vec<String>,
    source: String,
    attribute: String,
    terminal: bool,
}

impl LuluPublisher {
    /// Creates a new publisher for the given `source` and `attribute`.
    ///
    /// Source and attribute are validated exactly once here.
    /// Terminal output is disabled by default — call [`.terminal(true)`](Self::terminal)
    /// to enable it.
    ///
    /// # Errors
    /// Returns [`LuluError::InvalidSource`] or [`LuluError::InvalidAttribute`]
    /// if the parameters violate the lulu-logs topic naming rules.
    pub fn new(source: &str, attribute: &str) -> Result<Self, LuluError> {
        let source_segments = topic::parse_source(source)?;
        topic::validate_attribute(attribute)?;
        Ok(Self {
            source_segments,
            source: source.to_string(),
            attribute: attribute.to_string(),
            terminal: false,
        })
    }

    /// Enables or disables terminal output for this publisher.
    ///
    /// When enabled, each publish prints a coloured line to stdout:
    /// ```text
    /// [INFO ]  psu/channel-1 · voltage = Float32(3.31)
    /// [WARN ]  psu/channel-1 · voltage = Float32(2.80)
    /// [INFO ]  hello world
    /// ```
    pub fn terminal(mut self, enabled: bool) -> Self {
        self.terminal = enabled;
        self
    }

    fn publish(&self, level: LogLevel, data: Data) -> Result<(), LuluError> {
        if self.terminal {
            terminal_logger::print_publish(&self.source, &self.attribute, &level, &data);
        }
        let client = GLOBAL_CLIENT.get().ok_or(LuluError::NotInitialized)?;
        client.publish(PendingMessage {
            source_segments: self.source_segments.clone(),
            attribute: self.attribute.clone(),
            level,
            data,
        })
    }

    /// Publishes a `Trace`-level log entry.
    pub fn trace(&self, data: Data) -> Result<(), LuluError> {
        self.publish(LogLevel::Trace, data)
    }

    /// Publishes a `Debug`-level log entry.
    pub fn debug(&self, data: Data) -> Result<(), LuluError> {
        self.publish(LogLevel::Debug, data)
    }

    /// Publishes an `Info`-level log entry.
    pub fn info(&self, data: Data) -> Result<(), LuluError> {
        self.publish(LogLevel::Info, data)
    }

    /// Publishes a `Warn`-level log entry.
    pub fn warn(&self, data: Data) -> Result<(), LuluError> {
        self.publish(LogLevel::Warn, data)
    }

    /// Publishes an `Error`-level log entry.
    pub fn error(&self, data: Data) -> Result<(), LuluError> {
        self.publish(LogLevel::Error, data)
    }

    /// Publishes a `Fatal`-level log entry.
    pub fn fatal(&self, data: Data) -> Result<(), LuluError> {
        self.publish(LogLevel::Fatal, data)
    }
}
