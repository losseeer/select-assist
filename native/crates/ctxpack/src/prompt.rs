//! assemblePrompt：整条 Prompt 的预算，裁剪顺序是「先丢最旧上下文轮 → 再截选区 → 最后硬切」。
//! 对照 packages/ctxpack/src/prompt.ts。

use crate::utf16;

pub struct AssembleInput<'a> {
    /// 含 {selection} 时按占位填充；不含时用一个空行接在指令后面
    pub template: &'a str,
    pub selection: &'a str,
    /// 渲染好的上下文包（没挂上下文时是空串）
    pub context: &'a str,
    pub max_chars: usize,
}

pub struct Assembled {
    pub prompt: String,
    pub dropped: Vec<String>,
}

pub fn assemble_prompt(input: &AssembleInput) -> Assembled {
    let mut dropped: Vec<String> = Vec::new();
    let tpl = if input.template.contains("{selection}") {
        input.template.to_string()
    } else {
        format!("{}\n\n{{selection}}", input.template)
    };
    let fill = |sel: &str, ctx: &str| -> String {
        let instruction = tpl.replace("{selection}", sel);
        if ctx.is_empty() {
            instruction
        } else {
            format!("{instruction}\n\n{ctx}")
        }
    };

    let mut ctx = input.context.to_string();
    let mut sel = input.selection.to_string();
    if utf16::len(&fill(&sel, &ctx)) > input.max_chars {
        let mut turns: Vec<String> = ctx
            .split("\n\n")
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect();
        while turns.len() > 1 && utf16::len(&fill(&sel, &turns.join("\n\n"))) > input.max_chars {
            turns.remove(0);
            if !dropped.iter().any(|d| d == "context:trimmed-oldest") {
                dropped.push("context:trimmed-oldest".to_string());
            }
        }
        ctx = turns.join("\n\n");
        let over = utf16::len(&fill(&sel, &ctx)) as i64 - input.max_chars as i64;
        if over > 0 {
            let keep = (utf16::len(&sel) as i64 - over - 1).max(0) as usize; // 1 个省略号
            sel = if keep > 0 {
                let mut cut = utf16::head(&sel, keep);
                cut.push('…');
                cut
            } else {
                "…".to_string()
            };
            dropped.push("selection:truncated".to_string());
        }
    }

    let mut prompt = fill(&sel, &ctx);
    if utf16::len(&prompt) > input.max_chars {
        // 预算小到只剩一条超长轮时的最后一招
        let mut cut = utf16::head(&prompt, input.max_chars.saturating_sub(1));
        cut.push('…');
        prompt = cut;
        dropped.push("final:hard-trim".to_string());
    }
    Assembled { prompt, dropped }
}
