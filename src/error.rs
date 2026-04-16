/// Errors returned by the lulu-logs-client public API.
#[derive(Debug)]
pub enum LuluError {
    /// `lulu_init()` has already been called.
    AlreadyInitialized,

    /// `lulu_init()` has not been called yet.
    NotInitialized,

    /// The publish queue is full (back-pressure).
    QueueFull,

    /// The source string is invalid (bad segment).
    InvalidSource(String),

    /// The attribute string is invalid.
    InvalidAttribute(String),

    /// The type string must not be empty.
    InvalidType,

    /// The embedded recorder failed to start.
    RecorderStartFailed,

    /// The embedded recorder failed to stop or write the output file.
    RecorderStopFailed,
}

impl std::fmt::Display for LuluError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LuluError::AlreadyInitialized => {
                write!(f, "lulu_init() has already been called")
            }
            LuluError::NotInitialized => {
                write!(f, "lulu_init() has not been called yet")
            }
            LuluError::QueueFull => {
                write!(f, "publish queue is full")
            }
            LuluError::InvalidSource(s) => {
                write!(f, "invalid source: {}", s)
            }
            LuluError::InvalidAttribute(s) => {
                write!(f, "invalid attribute: {}", s)
            }
            LuluError::InvalidType => {
                write!(f, "invalid type: type string must not be empty")
            }
            LuluError::RecorderStartFailed => {
                write!(f, "failed to start the embedded recorder")
            }
            LuluError::RecorderStopFailed => {
                write!(f, "failed to stop the recorder or write the output file")
            }
        }
    }
}

impl std::error::Error for LuluError {}
