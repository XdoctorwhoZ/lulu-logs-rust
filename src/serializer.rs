use anyhow::anyhow;
use chrono::SecondsFormat;
use flatbuffers::FlatBufferBuilder;

use crate::lulu_logs_generated::lulu_logs::{LogEntry, LogEntryArgs};
use crate::models::{Data, LogLevel};

/// Message en attente de sérialisation et de publication MQTT.
pub(crate) struct PendingMessage {
    pub source_segments: Vec<String>,
    pub attribute:       String,
    pub level:           LogLevel,
    pub data:            Data,
}

/// Taille maximale autorisée pour un payload FlatBuffers sérialisé (20 Ko).
pub(crate) const MAX_PAYLOAD_SIZE: usize = 20_480;

/// Sérialise un `PendingMessage` en buffer FlatBuffers.
///
/// Le timestamp est généré ici (heure d'émission réelle).
/// Retourne `Err` si le buffer dépasse `MAX_PAYLOAD_SIZE`.
pub(crate) fn serialize(msg: &PendingMessage) -> anyhow::Result<Vec<u8>> {
    let timestamp = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let mut builder = FlatBufferBuilder::with_capacity(512);

    let fbs_timestamp = builder.create_string(&timestamp);
    let encoded = msg.data.encode();
    let fbs_type = builder.create_string(msg.data.data_type().as_str());
    let fbs_data = builder.create_vector(&encoded);

    let entry = LogEntry::create(
        &mut builder,
        &LogEntryArgs {
            timestamp: Some(fbs_timestamp),
            level: msg.level.to_fbs(),
            type_: Some(fbs_type),
            data: Some(fbs_data),
        },
    );

    builder.finish(entry, None);

    let finished = builder.finished_data();
    let len = finished.len();
    if len > MAX_PAYLOAD_SIZE {
        return Err(anyhow!(
            "payload size {} exceeds MAX_PAYLOAD_SIZE {}",
            len,
            MAX_PAYLOAD_SIZE
        ));
    }

    Ok(finished.to_vec())
}
