//! buildPack：体积超限时「先裁每条、再丢最旧、最后才截选区」，每步都在 dropped 里留名。
//! 对照 packages/ctxpack/src/build.ts。

use crate::{time, types::*, utf16};

pub const DEFAULT_MAX_CHARS: usize = 8000;
/// 广度优先：先把每条压短，实在放不下才整条丢弃
const TURN_CAPS: [usize; 5] = [2000, 1000, 500, 250, 120];

#[derive(Debug)]
pub struct BuildInput {
    pub selection: Selection,
    pub capture: Capture,
    pub source: Option<Source>,
    pub transcript: Vec<TranscriptTurn>,
    pub max_chars: usize,
    pub template_id: String,
    /// adapter 无论如何都要丢的东西，例如 "tool-results"
    pub default_dropped: Vec<String>,
    pub generated_at: Option<String>,
}

impl BuildInput {
    pub fn new(selection: Selection, capture: Capture) -> Self {
        Self {
            selection,
            capture,
            source: None,
            transcript: Vec::new(),
            max_chars: DEFAULT_MAX_CHARS,
            template_id: "plain/v1".to_string(),
            default_dropped: Vec::new(),
            generated_at: None,
        }
    }
}

fn uniq(records: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(records.len());
    for record in records {
        if !out.contains(record) {
            out.push(record.clone());
        }
    }
    out
}

/// 与 JS 一致：空 transcript 时 to = length - 1 = -1，会拼出 "turns:0--1"。
/// 这是 TS 侧的既有行为，对拍要求原样复刻，不修。
fn range_label(from: i64, to: i64) -> String {
    if from == to {
        format!("turns:{from}")
    } else {
        format!("turns:{from}-{to}")
    }
}

fn cap_turn(turn: &TranscriptTurn, cap: usize) -> TranscriptTurn {
    if utf16::len(&turn.text) <= cap {
        return turn.clone();
    }
    let head = (cap - 1).div_ceil(2);
    let tail = (cap - 1) / 2;
    let mut capped = utf16::head(&turn.text, head);
    capped.push('…');
    capped.push_str(&utf16::tail(&turn.text, tail));
    TranscriptTurn {
        text: capped,
        ..turn.clone()
    }
}

fn draft(
    input: &BuildInput,
    generated_at: &str,
    kept: &[TranscriptTurn],
    max_chars: usize,
    dropped: &[String],
) -> CtxPack {
    CtxPack {
        pack: PACK_FORMAT.to_string(),
        generated_at: generated_at.to_string(),
        capture: input.capture.clone(),
        source: input.source.clone(),
        selection: Some(input.selection.clone()),
        transcript: Some(kept.to_vec()),
        limits: Some(Limits {
            max_chars: Some(max_chars),
            used_chars: Some(0),
            dropped: Some(dropped.to_vec()),
        }),
        payload: None,
        extra: Default::default(),
    }
}

pub fn build_pack(input: &BuildInput) -> Result<CtxPack, String> {
    if input.selection.text.is_empty() {
        return Err("ctxpack: selection.text is required".to_string());
    }
    if input.capture.via.is_empty() || input.capture.at.is_empty() {
        return Err("ctxpack: capture.via/at are required".to_string());
    }

    let generated_at = input.generated_at.clone().unwrap_or_else(time::now_iso);
    let numbered: Vec<TranscriptTurn> = input
        .transcript
        .iter()
        .enumerate()
        .map(|(i, t)| TranscriptTurn {
            seq: Some(t.seq.unwrap_or(i)),
            ..t.clone()
        })
        .collect();

    let max_turn_len = numbered
        .iter()
        .map(|t| utf16::len(&t.text))
        .max()
        .unwrap_or(0);
    let caps: Vec<usize> = TURN_CAPS
        .into_iter()
        .filter(|c| *c < max_turn_len)
        .collect();

    // 尝试顺序就是语义：不裁 → 逐档裁 → 不裁但丢最旧 → 又裁又丢
    let mut attempts: Vec<(usize, usize)> = vec![(0, 0)];
    attempts.extend(caps.iter().map(|cap| (*cap, 0usize)));
    for drop_from in 1..numbered.len() {
        attempts.push((0, drop_from));
    }
    for cap in &caps {
        for drop_from in 1..numbered.len() {
            attempts.push((*cap, drop_from));
        }
    }

    for (cap, drop_from) in attempts {
        let turns: Vec<TranscriptTurn> = if cap > 0 {
            numbered.iter().map(|t| cap_turn(t, cap)).collect()
        } else {
            numbered.clone()
        };
        let kept = &turns[drop_from..];
        let mut dropped = input.default_dropped.clone();
        if cap > 0 {
            dropped.push(format!("turns:capped-{cap}"));
        }
        if drop_from > 0 {
            dropped.push(range_label(0, drop_from as i64 - 1));
        }

        let mut pack = draft(input, &generated_at, kept, input.max_chars, &dropped);
        let payload = crate::render::render(&pack, &input.template_id, &dropped);
        if utf16::len(&payload) <= input.max_chars {
            pack.limits = Some(Limits {
                max_chars: Some(input.max_chars),
                used_chars: Some(utf16::len(&payload)),
                dropped: Some(dropped),
            });
            pack.payload = Some(payload);
            return Ok(pack);
        }
    }

    // 选区本身就超预算：显式截断，绝不静默
    let mut dropped = input.default_dropped.clone();
    dropped.push(range_label(0, numbered.len() as i64 - 1));
    dropped.push("selection:truncated".to_string());
    let dropped = uniq(&dropped);

    let mut pack = draft(input, &generated_at, &[], input.max_chars, &dropped);
    let overhead = {
        let bare = CtxPack {
            selection: Some(Selection {
                text: String::new(),
                ..pack.selection.clone().unwrap()
            }),
            ..pack.clone()
        };
        let payload = crate::render::render(&bare, &input.template_id, &dropped);
        utf16::len(&payload)
    };
    let room = input.max_chars.saturating_sub(overhead + 1); // 1 个省略号
    let text = if room > 1 {
        let mut cut = utf16::head(&input.selection.text, room - 1);
        cut.push('…');
        cut
    } else {
        utf16::head(&input.selection.text, room)
    };
    pack.selection = Some(Selection {
        text,
        ..input.selection.clone()
    });
    let payload = crate::render::render(&pack, &input.template_id, &dropped);
    pack.limits = Some(Limits {
        max_chars: Some(input.max_chars),
        used_chars: Some(utf16::len(&payload)),
        dropped: Some(dropped),
    });
    pack.payload = Some(payload);
    Ok(pack)
}
