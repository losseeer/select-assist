//! 与 packages/ctxpack/test/ctxpack.test.mjs 一一对应的行为规格（18 例）。

use std::path::Path;

use serde_json::json;

use crate::ctxpack::build::{build_pack, BuildInput};
use crate::ctxpack::prompt::Assembled;
use crate::ctxpack::prompt::{assemble_prompt, AssembleInput};
use crate::ctxpack::redact::redact_paths;
use crate::ctxpack::types::CtxPack;
use crate::ctxpack::types::{Capture, Limits, Selection, Source, TranscriptTurn};
use crate::ctxpack::utf16;
use crate::ctxpack::validate::validate_pack;

fn capture() -> Capture {
    Capture {
        via: "clipboard".to_string(),
        at: "2026-09-24T10:12:01Z".to_string(),
    }
}

fn input(text: &str) -> BuildInput {
    BuildInput::new(
        Selection {
            text: text.to_string(),
            role: None,
            anchor: None,
        },
        capture(),
    )
}

fn dropped_of(pack: &CtxPack) -> Vec<String> {
    pack.limits
        .as_ref()
        .and_then(|l| l.dropped.clone())
        .unwrap_or_default()
}

fn used_of(pack: &CtxPack) -> usize {
    pack.limits.as_ref().and_then(|l| l.used_chars).unwrap_or(0)
}

fn payload_of(pack: &CtxPack) -> String {
    pack.payload.clone().unwrap_or_default()
}

fn build(input: BuildInput) -> CtxPack {
    build_pack(&input).expect("buildPack 不该失败")
}

#[test]
fn minimal_pack_selection_and_capture_only() {
    let pack = build(input("waterfall 监听器必须调用 next()"));
    assert_eq!(pack.pack, "ctxpack/0");
    assert_eq!(pack.transcript.clone().unwrap().len(), 0);
    let payload = payload_of(&pack);
    assert!(payload.contains("waterfall 监听器必须调用 next()"));
    assert!(!payload.contains("会话摘录"));
    assert!(validate_pack(&json!(pack)).ok);
}

#[test]
fn pack_with_transcript_validates_and_renders_context_block() {
    let mut request = input("sel");
    request.source = Some(Source {
        agent: Some("deepseek-harness".into()),
        app: Some("web".into()),
        session_id: Some("s-8f2".into()),
        adapter: Some("session-export-zip@0".into()),
        ..Default::default()
    });
    request.transcript = vec![
        TranscriptTurn {
            role: "user".into(),
            text: "第一条".into(),
            seq: Some(12),
        },
        TranscriptTurn {
            role: "assistant".into(),
            text: "第二条".into(),
            seq: Some(13),
        },
    ];
    let pack = build(request);
    assert!(validate_pack(&json!(pack)).ok);
    let payload = payload_of(&pack);
    assert!(payload.contains("会话摘录"));
    assert!(payload.contains("用户> 第一条"));
    assert_eq!(used_of(&pack), utf16::len(&payload));
    assert!(used_of(&pack) <= pack.limits.as_ref().and_then(|l| l.max_chars).unwrap());
}

#[test]
fn overflow_drops_oldest_turns_first_and_records_it() {
    let request = BuildInput {
        transcript: (0..40)
            .map(|i| TranscriptTurn {
                role: if i % 2 == 0 {
                    "user".into()
                } else {
                    "assistant".into()
                },
                text: format!("turn-{i} {}", "x".repeat(100)),
                seq: Some(i),
            })
            .collect(),
        max_chars: 1200,
        ..input(&"S".repeat(200))
    };
    let pack = build(request);
    let payload = payload_of(&pack);
    assert!(utf16::len(&payload) <= 1200);
    assert!(
        dropped_of(&pack).iter().any(|d| d.starts_with("turns:0-")),
        "{:?}",
        dropped_of(&pack)
    );
    assert!(payload.contains("turn-39 xxxxx"), "最新的轮必须活下来");
    assert!(
        payload.contains("为控制体积，本包已省略"),
        "省略要在正文里看得见"
    );
    assert!(validate_pack(&json!(pack)).ok);
}

#[test]
fn oversized_selection_is_truncated_explicitly() {
    let request = BuildInput {
        max_chars: 500,
        ..input(&"y".repeat(5000))
    };
    let pack = build(request);
    assert!(utf16::len(&payload_of(&pack)) <= 500);
    assert!(dropped_of(&pack).contains(&"selection:truncated".to_string()));
}

#[test]
fn long_turns_are_capped_breadth_first_before_being_dropped() {
    let request = BuildInput {
        transcript: (0..30)
            .map(|i| TranscriptTurn {
                role: if i % 2 == 0 {
                    "user".into()
                } else {
                    "assistant".into()
                },
                text: format!("T{i} ") + &"x".repeat(3000),
                seq: Some(i),
            })
            .collect(),
        max_chars: 8000,
        template_id: "clean/v1".to_string(),
        ..input("sel")
    };
    let pack = build(request);
    assert_eq!(
        pack.transcript.as_ref().unwrap().len(),
        30,
        "30 轮都要以裁短的形式留下"
    );
    let payload = payload_of(&pack);
    assert!(utf16::len(&payload) <= 8000);
    assert!(dropped_of(&pack)
        .iter()
        .any(|d| d.starts_with("turns:capped-")));
    assert!(payload.contains("T0 "), "最旧的轮也要还有影子");
    assert!(payload.contains('…'), "裁短要有可见标记");
    assert!(validate_pack(&json!(pack)).ok);
}

#[test]
fn capping_falls_back_to_dropping_oldest() {
    let request = BuildInput {
        transcript: (0..100)
            .map(|i| TranscriptTurn {
                role: if i % 2 == 0 {
                    "user".into()
                } else {
                    "assistant".into()
                },
                text: format!("T{i} ") + &"x".repeat(3000),
                seq: Some(i),
            })
            .collect(),
        max_chars: 2000,
        template_id: "clean/v1".to_string(),
        ..input("sel")
    };
    let pack = build(request);
    let payload = payload_of(&pack);
    assert!(utf16::len(&payload) <= 2000);
    assert!(pack.transcript.as_ref().unwrap().len() < 100);
    assert!(dropped_of(&pack).iter().any(|d| d.starts_with("turns:")));
    assert!(payload.contains("T99"), "新轮优先");
}

#[test]
fn adapter_default_drops_flow_into_payload() {
    let request = BuildInput {
        default_dropped: vec!["tool-results".into(), "assistant-thinking".into()],
        ..input("a")
    };
    let pack = build(request);
    assert!(dropped_of(&pack).contains(&"tool-results".to_string()));
    assert!(payload_of(&pack).contains("tool-results"));
}

#[test]
fn validate_rejects_bad_packs() {
    assert!(!validate_pack(&json!(null)).ok);
    assert!(!validate_pack(&json!({})).ok);
    assert!(
        validate_pack(&json!({
            "pack": "ctxpack/0", "generatedAt": "x",
            "capture": { "via": "clipboard", "at": "x" }, "selection": { "text": "ok" }
        }))
        .ok
    );
    let bad = validate_pack(&json!({
        "pack": "ctxpack/0", "generatedAt": "x",
        "capture": { "via": "telepathy", "at": "x" },
        "selection": { "text": "ok" },
        "transcript": [{ "role": "tool", "text": "x" }]
    }));
    assert!(!bad.ok);
    assert!(
        bad.errors.iter().any(|e| e.contains("capture.via")),
        "{:?}",
        bad.errors
    );
    assert!(
        bad.errors.iter().any(|e| e.contains("transcript[0].role")),
        "{:?}",
        bad.errors
    );
}

#[test]
fn unknown_fields_are_tolerated_for_forward_compatibility() {
    let value = json!({
        "pack": "ctxpack/1", "generatedAt": "x",
        "capture": { "via": "clipboard", "at": "x" },
        "selection": { "text": "ok" }, "someFutureField": 42
    });
    assert!(validate_pack(&value).ok);
    // 反序列化也必须带得回去
    let pack: CtxPack = serde_json::from_value(value).unwrap();
    assert_eq!(pack.extra.get("someFutureField"), Some(&json!(42)));
}

#[test]
fn render_markdown_template_is_selectable() {
    let request = BuildInput {
        template_id: "markdown/v1".to_string(),
        ..input("a\nb")
    };
    let payload = payload_of(&build(request));
    assert!(payload.starts_with("【上下文包】"));
    assert!(payload.contains("> a\n> b"), "{payload}");
}

#[test]
fn render_clean_v1_is_a_pure_record() {
    let request = BuildInput {
        template_id: "clean/v1".to_string(),
        transcript: vec![
            TranscriptTurn {
                role: "user".into(),
                text: "这个怎么理解？".into(),
                seq: None,
            },
            TranscriptTurn {
                role: "assistant".into(),
                text: "分两层看。".into(),
                seq: None,
            },
        ],
        ..input("选中的词")
    };
    let payload = payload_of(&build(request));
    assert_eq!(payload, "用户> 这个怎么理解？\n\n助手> 分两层看。");
    assert!(!payload.contains("上下文包"));
    assert!(!payload.contains("选区原文"));
    let solo = payload_of(&build(BuildInput {
        template_id: "clean/v1".into(),
        ..input("只有选区")
    }));
    assert_eq!(solo, "只有选区");
}

#[test]
fn redact_paths_hides_home_dir_in_payload_and_source() {
    let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
    let request = BuildInput {
        source: Some(Source {
            project_path: Some(format!("{home}/secret")),
            ..Default::default()
        }),
        ..input(&format!("see {home}/secret/file.ts"))
    };
    let pack = build(request);
    let redacted = redact_paths(&pack, &home);
    assert!(!payload_of(&redacted).contains(&home));
    assert_eq!(
        redacted.source.unwrap().project_path.as_deref(),
        Some("~/secret")
    );
}

#[test]
fn redact_paths_handles_windows_and_other_users() {
    let packed = CtxPack {
        payload: Some(r#"C:\Users\bob\prj\a.ts 与 /Users/alice/x 和 /Users/bob/y"#.to_string()),
        ..Default::default()
    };
    let out = payload_of(&redact_paths(&packed, "/nonexistent-home"));
    assert_eq!(out, r#"~\user\prj\a.ts 与 ~/user/x 和 ~/user/y"#);
}

const TPL: &str = "解释选中的词：「{selection}」";

fn assemble<'a>(
    template: &'a str,
    selection: &'a str,
    context: &'a str,
    max_chars: usize,
) -> Assembled {
    assemble_prompt(&AssembleInput {
        template,
        selection,
        context,
        max_chars,
    })
}

#[test]
fn assemble_fits_within_budget_untouched() {
    let r = assemble(TPL, "水", "用户> a\n\n助手> b", 8000);
    assert_eq!(r.prompt, "解释选中的词：「水」\n\n用户> a\n\n助手> b");
    assert!(r.dropped.is_empty());
}

#[test]
fn assemble_without_context_keeps_instruction_only() {
    assert_eq!(assemble(TPL, "水", "", 8000).prompt, "解释选中的词：「水」");
}

#[test]
fn assemble_appends_selection_when_template_has_no_placeholder() {
    assert_eq!(
        assemble("看看这个", "水", "", 8000).prompt,
        "看看这个\n\n水"
    );
}

#[test]
fn assemble_trims_oldest_context_before_touching_selection() {
    let context = format!(
        "用户> {}\n\n助手> {}\n\n助手> 最新一轮",
        "x".repeat(60),
        "y".repeat(60)
    );
    let r = assemble(TPL, "关键词", &context, 120);
    assert!(r.dropped.contains(&"context:trimmed-oldest".to_string()));
    assert!(r.prompt.contains("最新一轮"), "新轮要留下");
    assert!(!r.prompt.contains('x'), "最旧的轮先走");
    assert!(r.prompt.contains("关键词"), "选区优先保住");
    assert!(utf16::len(&r.prompt) <= 120);
}

#[test]
fn assemble_truncates_selection_with_a_visible_ellipsis() {
    let r = assemble(
        TPL,
        &"S".repeat(5000),
        &format!("助手> {}", "c".repeat(300)),
        800,
    );
    assert!(r.dropped.contains(&"selection:truncated".to_string()));
    assert!(r.prompt.contains("S…"), "截断要看得见");
    assert!(utf16::len(&r.prompt) <= 800);
    assert!(r.prompt.contains("助手> c"), "上下文优先于选区尾部");
}

#[test]
fn assemble_hard_caps_a_degenerate_budget() {
    let r = assemble(TPL, &"z".repeat(500), &"t".repeat(500), 40);
    assert!(utf16::len(&r.prompt) <= 40);
    assert!(!r.dropped.is_empty(), "绝不静默");
}

#[test]
fn limits_round_trip_through_json() {
    let limits = Limits {
        max_chars: Some(10),
        used_chars: Some(3),
        dropped: Some(vec!["x".into()]),
    };
    let value = serde_json::to_value(&limits).unwrap();
    assert_eq!(value["maxChars"], json!(10));
    assert_eq!(value["usedChars"], json!(3));
    assert_eq!(limits, serde_json::from_value(value).unwrap());
}

/// 跨实现对拍：同一批输入喂给 TS 版 ctxpack，把输出存成 native/fixtures/parity-ts.json，
/// Rust 侧必须逐字节一致（含 "turns:0--1" 这类既有怪癖，以及 emoji 的 UTF-16 计数）。
#[test]
fn matches_typescript_reference_output() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/parity-ts.json");
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(cases.len(), 9);
    let expected = |name: &str| -> &serde_json::Value {
        cases
            .iter()
            .find(|c| c["name"].as_str().unwrap() == name)
            .unwrap()
    };
    let check = |name: &str, pack: &CtxPack| {
        let want = expected(name);
        assert_eq!(
            payload_of(pack),
            want["payload"].as_str().unwrap(),
            "{name} payload"
        );
        assert_eq!(
            dropped_of(pack),
            want["dropped"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            "{name} dropped"
        );
        assert_eq!(
            Some(used_of(pack)),
            want["usedChars"].as_u64().map(|v| v as usize),
            "{name} usedChars"
        );
        let want_turns = want["transcript"].as_array().unwrap();
        assert_eq!(
            pack.transcript.as_ref().unwrap().len(),
            want_turns.len(),
            "{name} 轮数"
        );
        for (i, got) in pack.transcript.as_ref().unwrap().iter().enumerate() {
            assert_eq!(
                got.role,
                want_turns[i][0].as_str().unwrap(),
                "{name} turn{i} role"
            );
            assert_eq!(
                got.text,
                want_turns[i][1].as_str().unwrap(),
                "{name} turn{i} text"
            );
            assert_eq!(
                got.seq,
                want_turns[i][2].as_u64().map(|v| v as usize),
                "{name} turn{i} seq"
            );
        }
    };
    let check_prompt = |name: &str, got: &Assembled| {
        let want = expected(name);
        assert_eq!(
            got.prompt,
            want["payload"].as_str().unwrap(),
            "{name} prompt"
        );
        assert_eq!(
            got.dropped,
            want["dropped"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            "{name} dropped"
        );
    };

    check(
        "minimal",
        &build(BuildInput {
            generated_at: Some("FIXED".into()),
            ..input("waterfall 监听器必须调用 next()")
        }),
    );
    check(
        "clean-with-transcript",
        &build(BuildInput {
            template_id: "clean/v1".into(),
            generated_at: Some("FIXED".into()),
            transcript: vec![
                TranscriptTurn {
                    role: "user".into(),
                    text: "这个怎么理解？".into(),
                    seq: None,
                },
                TranscriptTurn {
                    role: "assistant".into(),
                    text: "分两层看。".into(),
                    seq: None,
                },
            ],
            ..input("选中的词")
        }),
    );
    check(
        "markdown",
        &build(BuildInput {
            template_id: "markdown/v1".into(),
            generated_at: Some("FIXED".into()),
            source: Some(Source {
                agent: Some("qoder".into()),
                app: Some("desktop".into()),
                session_id: Some("s1".into()),
                ..Default::default()
            }),
            transcript: vec![
                TranscriptTurn {
                    role: "user".into(),
                    text: "问".into(),
                    seq: None,
                },
                TranscriptTurn {
                    role: "assistant".into(),
                    text: "答".into(),
                    seq: None,
                },
            ],
            ..input("a\nb")
        }),
    );
    let overflow = BuildInput {
        transcript: (0..40)
            .map(|i| TranscriptTurn {
                role: if i % 2 == 0 {
                    "user".into()
                } else {
                    "assistant".into()
                },
                text: format!("turn-{i} {}", "x".repeat(100)),
                seq: Some(i),
            })
            .collect(),
        max_chars: 1200,
        generated_at: Some("FIXED".into()),
        ..input(&"S".repeat(200))
    };
    check("overflow-turns", &build(overflow));
    let capped = BuildInput {
        transcript: (0..30)
            .map(|i| TranscriptTurn {
                role: if i % 2 == 0 {
                    "user".into()
                } else {
                    "assistant".into()
                },
                text: format!("T{i} {}", "x".repeat(3000)),
                seq: Some(i),
            })
            .collect(),
        max_chars: 8000,
        template_id: "clean/v1".into(),
        generated_at: Some("FIXED".into()),
        ..input("sel")
    };
    check("capped-breadth", &build(capped));
    check(
        "selection-truncated",
        &build(BuildInput {
            max_chars: 500,
            generated_at: Some("FIXED".into()),
            ..input(&"y".repeat(5000))
        }),
    );
    check(
        "emoji-caps",
        &build(BuildInput {
            transcript: vec![TranscriptTurn {
                role: "user".into(),
                text: "👍".repeat(200),
                seq: None,
            }],
            max_chars: 300,
            template_id: "clean/v1".into(),
            generated_at: Some("FIXED".into()),
            ..input(&"👍".repeat(400))
        }),
    );
    check_prompt(
        "assemble-truncates-selection",
        &assemble(
            TPL,
            &"S".repeat(5000),
            &format!("助手> {}", "c".repeat(300)),
            800,
        ),
    );
    check_prompt(
        "assemble-trims-oldest",
        &assemble(
            "看看这个",
            "关键词",
            &format!(
                "用户> {}\n\n助手> {}\n\n助手> 最新一轮",
                "x".repeat(60),
                "y".repeat(60)
            ),
            120,
        ),
    );
}
