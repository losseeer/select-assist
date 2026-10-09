//! validatePack：契约只强制 selection.text 与 capture，其余可缺省、未知字段必须容忍。
//! 对照 packages/ctxpack/src/validate.ts（错误文案一字不差）。

use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct ValidationResult {
    pub ok: bool,
    pub errors: Vec<String>,
}

const CAPTURE_VIAS: [&str; 2] = ["page-selection", "clipboard"];
const ANCHOR_KINDS: [&str; 4] = ["event", "message", "ambiguous", "none"];
const ROLES: [&str; 2] = ["user", "assistant"];

/// 等价于 /^ctxpack\/\d+$/
fn is_pack_format(s: &str) -> bool {
    match s.strip_prefix("ctxpack/") {
        Some(rest) => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

pub fn validate_pack(value: &Value) -> ValidationResult {
    let mut errors: Vec<String> = Vec::new();
    let Some(p) = value.as_object() else {
        return ValidationResult {
            ok: false,
            errors: vec!["pack must be an object".to_string()],
        };
    };

    let is_obj = |v: Option<&Value>| v.map(Value::is_object).unwrap_or(false);

    match p.get("pack") {
        Some(Value::String(s)) if is_pack_format(s) => {}
        _ => errors.push("field \"pack\" must look like \"ctxpack/<major>\"".to_string()),
    }
    if !p.get("generatedAt").map(Value::is_string).unwrap_or(false) {
        errors.push("field \"generatedAt\" must be a string".to_string());
    }

    if !is_obj(p.get("capture")) {
        errors.push("field \"capture\" is required".to_string());
    } else if let Some(c) = p.get("capture").and_then(Value::as_object) {
        match c.get("via") {
            Some(Value::String(via)) if CAPTURE_VIAS.contains(&via.as_str()) => {}
            _ => errors.push("capture.via must be \"page-selection\" | \"clipboard\"".to_string()),
        }
        if !c.get("at").map(Value::is_string).unwrap_or(false) {
            errors.push("capture.at must be a string".to_string());
        }
    }

    let selection = p.get("selection");
    let selection_text_ok = selection
        .and_then(|s| s.as_object())
        .and_then(|s| s.get("text"))
        .map(Value::is_string)
        .unwrap_or(false);
    if !selection_text_ok {
        errors.push("field \"selection.text\" is required and must be a string".to_string());
    } else if let Some(s) = selection.and_then(Value::as_object) {
        match s.get("role") {
            None | Some(Value::Null) => {}
            Some(Value::String(role))
                if ["user", "assistant", "unknown"].contains(&role.as_str()) => {}
            _ => errors.push("selection.role must be user|assistant|unknown".to_string()),
        }
        if let Some(anchor) = s.get("anchor") {
            if !anchor.is_object() {
                errors.push("selection.anchor must be an object".to_string());
            } else {
                let kind = anchor
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let is_string = anchor.get("kind").map(Value::is_string).unwrap_or(false);
                if !is_string || !ANCHOR_KINDS.contains(&kind) {
                    errors.push(
                        "selection.anchor.kind must be event|message|ambiguous|none".to_string(),
                    );
                }
            }
        }
    }

    if let Some(transcript) = p.get("transcript") {
        match transcript.as_array() {
            None => errors.push("field \"transcript\" must be an array".to_string()),
            Some(turns) => {
                for (i, t) in turns.iter().enumerate() {
                    match t.as_object() {
                        None => errors.push(format!("transcript[{i}] must be an object")),
                        Some(tt) => {
                            if !tt.get("text").map(Value::is_string).unwrap_or(false) {
                                errors.push(format!("transcript[{i}].text must be a string"));
                            }
                            let role = tt.get("role").and_then(Value::as_str).unwrap_or_default();
                            let is_string = tt.get("role").map(Value::is_string).unwrap_or(false);
                            if !is_string || !ROLES.contains(&role) {
                                errors.push(format!("transcript[{i}].role must be user|assistant"));
                            }
                        }
                    }
                }
            }
        }
    }

    if p.get("limits").map(|v| !v.is_object()).unwrap_or(false) {
        errors.push("field \"limits\" must be an object".to_string());
    }
    if p.get("source").map(|v| !v.is_object()).unwrap_or(false) {
        errors.push("field \"source\" must be an object".to_string());
    }

    ValidationResult {
        ok: errors.is_empty(),
        errors,
    }
}
