//! ctxpack/0 的数据契约，字段名与 packages/ctxpack/src/types.ts 逐一对齐。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const PACK_FORMAT: &str = "ctxpack/0";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    /// "page-selection" | "clipboard"
    pub via: String,
    /// ISO 8601
    pub at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// "event" | "message" | "ambiguous" | "none"
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    pub text: String,
    /// "user" | "assistant" | "unknown"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranscriptTurn {
    /// "user" | "assistant"
    pub role: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_chars: Option<usize>,
    /// 每一次省略都要留名，禁止静默截断
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CtxPack {
    pub pack: String,
    pub generated_at: String,
    pub capture: Capture,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Vec<TranscriptTurn>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    /// 契约允许前向兼容的未知字段，读进来必须原样带回去
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
