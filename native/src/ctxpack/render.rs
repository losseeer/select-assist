//! 渲染模板，逐行对照 packages/ctxpack/src/render.ts。

use crate::ctxpack::types::{CtxPack, TranscriptTurn};

fn user_line(t: &TranscriptTurn) -> String {
    let speaker = if t.role == "user" { "用户" } else { "助手" };
    format!("{speaker}> {}", t.text)
}

fn header(pack: &CtxPack) -> String {
    let fallback = crate::ctxpack::types::Source::default();
    let source = pack.source.as_ref().unwrap_or(&fallback);
    let mut parts = vec![
        source
            .agent
            .clone()
            .unwrap_or_else(|| "unknown agent".to_string()),
        source.app.clone().unwrap_or_else(|| "?".to_string()),
    ];
    if let Some(session_id) = &source.session_id {
        parts.push(format!("session {session_id}"));
    }
    let via = if pack.capture.via == "clipboard" {
        "剪贴板"
    } else {
        "页面选区"
    };
    format!(
        "【上下文包】选中内容来自 {}，抓取于 {}（{via}）",
        parts.join(" · "),
        pack.capture.at
    )
}

fn context_block(pack: &CtxPack, heading: &dyn Fn(&str) -> String) -> String {
    let turns = pack.transcript.clone().unwrap_or_default();
    if turns.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = turns.iter().map(user_line).collect();
    let title = heading(&format!("会话摘录（最近 {} 条）", turns.len()));
    format!("\n\n{title}\n\n{}", lines.join("\n\n"))
}

fn dropped_block(dropped: &[String], heading: &dyn Fn(&str) -> String) -> String {
    if dropped.is_empty() {
        return String::new();
    }
    let items: Vec<String> = dropped.iter().map(|d| format!("- {d}")).collect();
    format!(
        "\n\n{}\n\n{}",
        heading("为控制体积，本包已省略"),
        items.join("\n")
    )
}

fn selection_text(pack: &CtxPack) -> String {
    pack.selection
        .as_ref()
        .map(|s| s.text.clone())
        .unwrap_or_default()
}

fn plain_v1(pack: &CtxPack, dropped: &[String]) -> String {
    let h = |t: &str| format!("—— {t} ——");
    let mut out = format!(
        "{}\n\n{}\n\n{}",
        header(pack),
        h("选区原文"),
        selection_text(pack)
    );
    out += &context_block(pack, &h);
    out += &dropped_block(dropped, &h);
    out += &format!("\n\n{}", h("选区原文结束，以下是我的问题"));
    out
}

fn markdown_v1(pack: &CtxPack, dropped: &[String]) -> String {
    let h = |t: &str| format!("### {t}");
    let quoted = selection_text(pack).replace('\n', "\n> ");
    let mut out = format!("{}\n\n{}\n\n> {}", header(pack), h("选区原文"), quoted);
    out += &context_block(pack, &|t: &str| format!("\n{}", h(t)))
        .replace("\n用户> ", "\n**用户**: ")
        .replace("\n助手> ", "\n**助手**: ");
    out += &dropped_block(dropped, &h);
    out += &format!("\n\n{}", h("以上为参考上下文，我的问题是："));
    out
}

/// 纯记录：只有会话行。没有 transcript 时就是选区本身 —— 直通模式的逐字节原文靠它
fn clean_v1(pack: &CtxPack, _dropped: &[String]) -> String {
    let turns = pack.transcript.clone().unwrap_or_default();
    if turns.is_empty() {
        return selection_text(pack);
    }
    turns.iter().map(user_line).collect::<Vec<_>>().join("\n\n")
}

pub fn render(pack: &CtxPack, template_id: &str, dropped: &[String]) -> String {
    match template_id {
        "plain/v1" => plain_v1(pack, dropped),
        "markdown/v1" => markdown_v1(pack, dropped),
        "clean/v1" => clean_v1(pack, dropped),
        other => panic!("unknown ctxpack template: {other}"),
    }
}
