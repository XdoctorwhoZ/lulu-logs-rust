use crate::lulu_logs_generated::lulu_logs::LogLevel as FbsLogLevel;

/// Niveau de sévérité — miroir de l'enum FlatBuffers `LuluLogs::LogLevel`.
/// L'ordre de déclaration est significatif (Trace = 0 ... Fatal = 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
}

impl LogLevel {
    /// Converts this LogLevel to the FlatBuffers-generated enum variant.
    pub fn to_fbs(&self) -> FbsLogLevel {
        match self {
            LogLevel::Trace => FbsLogLevel::Trace,
            LogLevel::Debug => FbsLogLevel::Debug,
            LogLevel::Info => FbsLogLevel::Info,
            LogLevel::Warn => FbsLogLevel::Warn,
            LogLevel::Error => FbsLogLevel::Error,
            LogLevel::Fatal => FbsLogLevel::Fatal,
        }
    }
}

// ---------------------------------------------------------------------------
// DataType
// ---------------------------------------------------------------------------

/// Type de donnée transporté dans le champ `data` d'un `LogEntry`.
///
/// Correspond aux valeurs reconnues définies dans le §3.3 de la spécification
/// lulu-logs v1.3.0. Utiliser cet enum plutôt qu'une chaîne brute évite les
/// fautes de frappe et rend l'API auto-documentée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    /// Chaîne de caractères encodée en UTF-8.
    String,
    /// Entier signé 32 bits, little-endian.
    Int32,
    /// Entier signé 64 bits, little-endian.
    Int64,
    /// Flottant simple précision IEEE 754, little-endian.
    Float32,
    /// Flottant double précision IEEE 754, little-endian.
    Float64,
    /// Booléen : 1 octet (`0x00` = false, `0x01` = true).
    Bool,
    /// Document JSON encodé en UTF-8.
    Json,
    /// Données binaires opaques, sans interprétation définie.
    Bytes,
    /// Paquet réseau opaque (données binaires brutes, §3.5).
    NetPacket,
    /// Fragment de liaison série opaque (données binaires brutes, §3.5).
    SerialChunk,
    /// Début de span générique — JSON UTF-8 avec `span_id` et `kind`.
    SpanBeg,
    /// Fin de span générique — JSON UTF-8 avec `span_id`, `kind` et `success`.
    SpanEnd,
    /// Début de scénario — dérivé spécialisé de `span_beg`.
    ScenarioBeg,
    /// Fin de scénario — dérivé spécialisé de `span_end`.
    ScenarioEnd,
    /// Début d'appel d'outil — dérivé spécialisé de `span_beg`.
    ToolCallBeg,
    /// Fin d'appel d'outil — dérivé spécialisé de `span_end`.
    ToolCallEnd,
    /// Début d'étape de test — dérivé spécialisé de `span_beg`.
    StepBeg,
    /// Fin d'étape de test — dérivé spécialisé de `span_end`.
    StepEnd,
}

impl DataType {
    /// Returns the protocol wire string for this data type (as defined in §3.3).
    pub fn as_str(&self) -> &'static str {
        match self {
            DataType::String => "string",
            DataType::Int32 => "int32",
            DataType::Int64 => "int64",
            DataType::Float32 => "float32",
            DataType::Float64 => "float64",
            DataType::Bool => "bool",
            DataType::Json => "json",
            DataType::Bytes => "bytes",
            DataType::NetPacket => "net_packet",
            DataType::SerialChunk => "serial_chunk",
            DataType::SpanBeg => "span_beg",
            DataType::SpanEnd => "span_end",
            DataType::ScenarioBeg => "scenario_beg",
            DataType::ScenarioEnd => "scenario_end",
            DataType::ToolCallBeg => "tool_call_beg",
            DataType::ToolCallEnd => "tool_call_end",
            DataType::StepBeg => "step_beg",
            DataType::StepEnd => "step_end",
        }
    }

    // ── Encoding helpers ────────────────────────────────────────────────────
    //
    // Each function encodes a typed Rust value into the raw `Vec<u8>` expected
    // by the `data` field of a `LogEntry`, following the encodings defined in
    // §3.3 of the lulu-logs specification.

    /// Encodes a UTF-8 string (use with [`DataType::String`]).
    pub fn encode_string(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    /// Encodes a signed 32-bit integer as little-endian bytes (use with [`DataType::Int32`]).
    pub fn encode_int32(v: i32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    /// Encodes a signed 64-bit integer as little-endian bytes (use with [`DataType::Int64`]).
    pub fn encode_int64(v: i64) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    /// Encodes an IEEE 754 single-precision float as little-endian bytes (use with [`DataType::Float32`]).
    pub fn encode_float32(v: f32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    /// Encodes an IEEE 754 double-precision float as little-endian bytes (use with [`DataType::Float64`]).
    pub fn encode_float64(v: f64) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    /// Encodes a boolean as a single byte: `0x01` = true, `0x00` = false (use with [`DataType::Bool`]).
    pub fn encode_bool(v: bool) -> Vec<u8> {
        vec![v as u8]
    }

    /// Encodes a JSON document string as UTF-8 bytes (use with [`DataType::Json`]).
    pub fn encode_json(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    /// Returns opaque bytes as-is (use with [`DataType::Bytes`]).
    pub fn encode_bytes(v: Vec<u8>) -> Vec<u8> {
        v
    }

    /// Returns opaque bytes as-is (use with [`DataType::NetPacket`]).
    pub fn encode_net_packet(v: Vec<u8>) -> Vec<u8> {
        v
    }

    /// Returns opaque bytes as-is (use with [`DataType::SerialChunk`]).
    pub fn encode_serial_chunk(v: Vec<u8>) -> Vec<u8> {
        v
    }
}

impl std::fmt::Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------

/// A typed log payload — carries both the type identity and the native Rust value.
///
/// Using this enum instead of a separate `(DataType, Vec<u8>)` pair ensures that the
/// type tag and the encoded bytes can never disagree at the call site.
///
/// # Example
/// ```rust
/// use lulu_logs_client::Data;
///
/// let d = Data::Float32(3.3);
/// let (wire_type, bytes) = (d.data_type(), d.encode());
/// ```
#[derive(Debug, Clone)]
pub enum Data {
    /// UTF-8 string.
    String(std::string::String),
    /// Signed 32-bit integer, encoded little-endian.
    Int32(i32),
    /// Signed 64-bit integer, encoded little-endian.
    Int64(i64),
    /// IEEE 754 single-precision float, encoded little-endian.
    Float32(f32),
    /// IEEE 754 double-precision float, encoded little-endian.
    Float64(f64),
    /// Boolean: `0x01` = true, `0x00` = false.
    Bool(bool),
    /// JSON document encoded as UTF-8.
    Json(std::string::String),
    /// Opaque binary bytes.
    Bytes(Vec<u8>),
    /// Opaque binary data containing a network packet (§3.5).
    NetPacket(Vec<u8>),
    /// Opaque binary data containing a serial link chunk (§3.5).
    SerialChunk(Vec<u8>),
    /// Generic span begin payload encoded as UTF-8 JSON.
    SpanBeg(std::string::String),
    /// Generic span end payload encoded as UTF-8 JSON.
    SpanEnd(std::string::String),
    /// Specialized scenario begin payload encoded as UTF-8 JSON.
    ScenarioBeg(std::string::String),
    /// Specialized scenario end payload encoded as UTF-8 JSON.
    ScenarioEnd(std::string::String),
    /// Specialized tool-call begin payload encoded as UTF-8 JSON.
    ToolCallBeg(std::string::String),
    /// Specialized tool-call end payload encoded as UTF-8 JSON.
    ToolCallEnd(std::string::String),
    /// Specialized step begin payload encoded as UTF-8 JSON.
    StepBeg(std::string::String),
    /// Specialized step end payload encoded as UTF-8 JSON.
    StepEnd(std::string::String),
}

impl Data {
    /// Returns the [`DataType`] tag that corresponds to this variant.
    pub fn data_type(&self) -> DataType {
        match self {
            Data::String(_) => DataType::String,
            Data::Int32(_) => DataType::Int32,
            Data::Int64(_) => DataType::Int64,
            Data::Float32(_) => DataType::Float32,
            Data::Float64(_) => DataType::Float64,
            Data::Bool(_) => DataType::Bool,
            Data::Json(_) => DataType::Json,
            Data::Bytes(_) => DataType::Bytes,
            Data::NetPacket(_) => DataType::NetPacket,
            Data::SerialChunk(_) => DataType::SerialChunk,
            Data::SpanBeg(_) => DataType::SpanBeg,
            Data::SpanEnd(_) => DataType::SpanEnd,
            Data::ScenarioBeg(_) => DataType::ScenarioBeg,
            Data::ScenarioEnd(_) => DataType::ScenarioEnd,
            Data::ToolCallBeg(_) => DataType::ToolCallBeg,
            Data::ToolCallEnd(_) => DataType::ToolCallEnd,
            Data::StepBeg(_) => DataType::StepBeg,
            Data::StepEnd(_) => DataType::StepEnd,
        }
    }

    /// Encodes the carried value into raw bytes following the §3.3 wire format.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Data::String(s) => DataType::encode_string(s),
            Data::Int32(v) => DataType::encode_int32(*v),
            Data::Int64(v) => DataType::encode_int64(*v),
            Data::Float32(v) => DataType::encode_float32(*v),
            Data::Float64(v) => DataType::encode_float64(*v),
            Data::Bool(v) => DataType::encode_bool(*v),
            Data::Json(s) => DataType::encode_json(s),
            Data::Bytes(v) => v.clone(),
            Data::NetPacket(v) => v.clone(),
            Data::SerialChunk(v) => v.clone(),
            Data::SpanBeg(s) => DataType::encode_json(s),
            Data::SpanEnd(s) => DataType::encode_json(s),
            Data::ScenarioBeg(s) => DataType::encode_json(s),
            Data::ScenarioEnd(s) => DataType::encode_json(s),
            Data::ToolCallBeg(s) => DataType::encode_json(s),
            Data::ToolCallEnd(s) => DataType::encode_json(s),
            Data::StepBeg(s) => DataType::encode_json(s),
            Data::StepEnd(s) => DataType::encode_json(s),
        }
    }
}

impl std::fmt::Display for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.data_type().fmt(f)
    }
}
