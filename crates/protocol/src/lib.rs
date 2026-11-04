//! Versioned wire types shared by the server, desktop and future mobile clients.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use validator::Validate;

/// Parent UUIDs remain stable for speech/chat ownership and foreign references.
/// One-to-one children use that UUID as a local key, but need independent cloud
/// identities because cloud records share one (account, id) primary key.
pub fn sync_record_id(account: Uuid, table: &str, local_id: &str) -> Uuid {
    if !matches!(
        table,
        "conversation_state" | "meeting_recaps" | "meeting_sentiment"
    ) && let Ok(id) = Uuid::parse_str(local_id)
    {
        return id;
    }
    Uuid::new_v5(&account, format!("{table}:{local_id}").as_bytes())
}

pub const API_VERSION: u16 = 1;
pub const MAX_RECORD_BYTES: usize = 256 * 1024;
pub const MAX_BATCH: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Conversation,
    Message,
    Meeting,
    Transcript,
    Prompt,
    Profile,
    Settings,
    Attachment,
    Recap,
    Context,
    Note,
    Speaker,
    Summary,
    PromptAttachment,
    MeetingConversation,
    Analysis,
}
impl RecordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Message => "message",
            Self::Meeting => "meeting",
            Self::Transcript => "transcript",
            Self::Prompt => "prompt",
            Self::Profile => "profile",
            Self::Settings => "settings",
            Self::Attachment => "attachment",
            Self::Recap => "recap",
            Self::Context => "context",
            Self::Note => "note",
            Self::Speaker => "speaker",
            Self::Summary => "summary",
            Self::PromptAttachment => "prompt_attachment",
            Self::MeetingConversation => "meeting_conversation",
            Self::Analysis => "analysis",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mutation {
    pub mutation_id: Uuid,
    pub id: Uuid,
    pub kind: RecordKind,
    /// Optimistic concurrency: 0 creates a record, otherwise match its server revision.
    pub base_revision: i64,
    pub deleted: bool,
    pub data: Value,
}
impl Mutation {
    pub fn validate_record(&self) -> Result<(), &'static str> {
        if self.base_revision < 0 {
            return Err("negative revision");
        }
        if !self.data.is_object() || self.data.to_string().len() > MAX_RECORD_BYTES {
            return Err("record must be an object of at most 256 KiB");
        }
        // Sync preferences, never credentials or device-specific settings.
        if self.kind == RecordKind::Settings {
            const ALLOWED: &[&str] = &[
                "theme",
                "response_length",
                "response_language",
                "chat_model",
                "listen_mode",
                "auto_answer",
                "answer_pacing",
            ];
            if self
                .data
                .as_object()
                .unwrap()
                .keys()
                .any(|key| !ALLOWED.contains(&key.as_str()))
            {
                return Err("unsupported synced preference");
            }
        }
        if forbidden_keys(&self.data) {
            return Err("credentials and raw audio cannot be synced");
        }
        Ok(())
    }
}

fn forbidden_keys(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let key = key.to_ascii_lowercase();
            key.contains("api_key")
                || key.contains("secret")
                || key.contains("password")
                || key.contains("access_token")
                || key.contains("refresh_token")
                || key == "raw_audio"
                || forbidden_keys(value)
        }),
        Value::Array(array) => array.iter().any(forbidden_keys),
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub sequence: i64,
    pub id: Uuid,
    pub kind: RecordKind,
    pub revision: i64,
    pub deleted: bool,
    pub data: Value,
    pub device_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub mutations: Vec<Mutation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResult {
    pub mutation_id: Uuid,
    pub change: Change,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    pub changes: Vec<Change>,
    pub cursor: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct AuthStart {
    pub device_id: Uuid,
    #[validate(length(min = 1, max = 80))]
    pub device_name: String,
    #[validate(length(equal = 43))]
    pub code_challenge: String,
    /// Desktop loopback, or a server-configured mobile universal link.
    pub redirect_uri: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthExchange {
    pub ticket: String,
    pub code_verifier: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub account_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mutation(kind: RecordKind, data: Value) -> Mutation {
        Mutation {
            mutation_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            kind,
            base_revision: 0,
            deleted: false,
            data,
        }
    }
    #[test]
    fn credentials_and_unknown_preferences_are_rejected() {
        assert!(
            mutation(
                RecordKind::Settings,
                serde_json::json!({"api_key":"secret"})
            )
            .validate_record()
            .is_err()
        );
        assert!(
            mutation(
                RecordKind::Settings,
                serde_json::json!({"microphone":"device"})
            )
            .validate_record()
            .is_err()
        );
        assert!(
            mutation(RecordKind::Settings, serde_json::json!({"theme":"dark"}))
                .validate_record()
                .is_ok()
        );
        assert!(
            mutation(RecordKind::Meeting, serde_json::json!({"raw_audio":"..."}))
                .validate_record()
                .is_err()
        );
    }
    #[test]
    fn bounds_and_wire_names() {
        assert!(
            mutation(
                RecordKind::Message,
                serde_json::json!({"content":"a".repeat(MAX_RECORD_BYTES)})
            )
            .validate_record()
            .is_err()
        );
        assert_eq!(
            serde_json::to_value(RecordKind::Transcript).unwrap(),
            "transcript"
        );
    }
}
