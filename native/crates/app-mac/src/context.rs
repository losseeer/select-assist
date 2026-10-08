//! 会话上下文：发现 → 选会话 → 读 transcript → clean/v1 渲染 → assemblePrompt。
//! 对照 packages/panel/src/main/capture.ts（userCandidates / attachContext / currentPayload）。

use ctxpack::adapters::claude_code;
use ctxpack::adapters::codex::Codex;
use ctxpack::adapters::qoder;
use ctxpack::adapters::util;
use ctxpack::adapters::workbuddy::Workbuddy;
use ctxpack::adapters::{mtime_ms, Adapter, DiscoverOpts, SessionRef};
use ctxpack::build::BuildInput;
use ctxpack::prompt::AssembleInput;
use ctxpack::types::{Capture, Selection, Source, TranscriptTurn};
use ctxpack::utf16;
use ctxpack::{build_pack, pick_session, redact_paths, render};
use settings::{active_template, AppSettings};

/// 轮数是唯一的裁剪旋钮：任何地方都不做字符截断
const NO_BUDGET: usize = usize::MAX;
const TEMPLATE: &str = "clean/v1";
/// 与 capture.ts 的 browse(limit = 40) 一致
pub const BROWSE_LIMIT: usize = 40;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AdapterKind {
    ClaudeCode,
    Codex,
    Workbuddy,
    QoderCn,
    QoderWork,
}

impl AdapterKind {
    pub fn id(self) -> &'static str {
        match self {
            AdapterKind::ClaudeCode => "claude-code-jsonl@0",
            AdapterKind::Codex => "codex-jsonl@0",
            AdapterKind::Workbuddy => "workbuddy-jsonl@0",
            AdapterKind::QoderCn => "qoder-cn-jsonl@0",
            AdapterKind::QoderWork => "qoderwork-sqlite@0",
        }
    }

    pub fn agent(self) -> &'static str {
        match self {
            AdapterKind::ClaudeCode => "claude-code",
            AdapterKind::Codex => "codex",
            AdapterKind::Workbuddy => "workbuddy",
            AdapterKind::QoderCn | AdapterKind::QoderWork => "qoder",
        }
    }

    /// 设置里 `agent|路径` 的 agent 段。qoder 一家两个存储，光看 agent 分不出来，
    /// 得看路径形态：.db = QoderWork，目录 = CN jsonl
    pub fn for_path(agent: &str, path: &str) -> Option<Self> {
        match agent {
            "claude-code" => Some(AdapterKind::ClaudeCode),
            "codex" => Some(AdapterKind::Codex),
            "workbuddy" => Some(AdapterKind::Workbuddy),
            "qoder" => Some(if path.replace('\\', "/").ends_with(".db") {
                AdapterKind::QoderWork
            } else {
                AdapterKind::QoderCn
            }),
            _ => None,
        }
    }

    fn discover(self, opts: &DiscoverOpts) -> Vec<SessionRef> {
        match self {
            AdapterKind::ClaudeCode => claude_code::adapter().discover(opts),
            AdapterKind::Codex => Codex.discover(opts),
            AdapterKind::Workbuddy => Workbuddy.discover(opts),
            AdapterKind::QoderCn => qoder::qoder_cn_adapter().discover(opts),
            AdapterKind::QoderWork => qoder::QoderWork.discover(opts),
        }
    }

    fn read(self, reference: &SessionRef) -> ctxpack::adapters::TranscriptResult {
        match self {
            AdapterKind::ClaudeCode => claude_code::adapter().read_transcript(reference),
            AdapterKind::Codex => Codex.read_transcript(reference),
            AdapterKind::Workbuddy => Workbuddy.read_transcript(reference),
            AdapterKind::QoderCn => qoder::qoder_cn_adapter().read_transcript(reference),
            AdapterKind::QoderWork => qoder::QoderWork.read_transcript(reference),
        }
    }
}

/// 路径形态决定归属：auto 条目靠这个分类
fn adapter_for_file(file: &str, hint: &str) -> Option<AdapterKind> {
    if hint != "auto" {
        if let Some(named) = AdapterKind::for_path(hint, file) {
            return Some(named);
        }
    }
    let normalized = file.replace('\\', "/");
    if normalized.contains("/.codex/") {
        return Some(AdapterKind::Codex);
    }
    if normalized.contains("/.workbuddy/") {
        return Some(AdapterKind::Workbuddy);
    }
    if normalized.contains("/.qoder-cn/") || normalized.contains("QoderWork") {
        return Some(if normalized.ends_with(".db") {
            AdapterKind::QoderWork
        } else {
            AdapterKind::QoderCn
        });
    }
    if normalized.ends_with(".jsonl") {
        return Some(AdapterKind::ClaudeCode);
    }
    None
}

fn base_name(file: &str) -> String {
    let last = file.rsplit(['/', '\\']).next().unwrap_or(file);
    last.strip_suffix(".jsonl")
        .or_else(|| last.strip_suffix(".db"))
        .unwrap_or(last)
        .to_string()
}

/// 单个文件条目：sessionId 只在文件名是 UUID 形状时给出
fn file_ref(kind: AdapterKind, file: &str, agent: Option<&str>) -> Option<SessionRef> {
    Some(SessionRef {
        agent: agent.unwrap_or(kind.agent()).to_string(),
        adapter: kind.id().to_string(),
        file_path: file.to_string(),
        session_id: util::uuid_like(&base_name(file)),
        mtime_ms: mtime_ms(file)?,
        ..Default::default()
    })
}

fn scan_jsonl(dir: &str, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path().to_string_lossy().to_string();
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() && depth > 0 {
            scan_jsonl(&path, depth - 1, out);
        } else if name.ends_with(".jsonl") {
            out.push(path);
        }
        if out.len() >= 200 {
            return; // 单条目录上限，防呆
        }
    }
}

/// 设置里的 `会话路径` 是唯一的发现来源
pub fn user_candidates(
    settings: &AppSettings,
    cwd: Option<&str>,
) -> Vec<(AdapterKind, SessionRef)> {
    let mut out: Vec<(AdapterKind, SessionRef)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for entry in &settings.session_paths {
        if entry.path.is_empty() || entry.agent == "project" {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&entry.path) else {
            continue; // 路径失效：跳过，不打断发现流程
        };
        let named = AdapterKind::for_path(&entry.agent, &entry.path);
        if let Some(kind) = named {
            if meta.is_file() && entry.path.ends_with(".jsonl") {
                if let Some(reference) = file_ref(kind, &entry.path, None) {
                    push_candidate(&mut out, &mut seen, kind, reference);
                }
            } else {
                // 目录和 .db 都交给 adapter 自己按 root 解释
                for reference in kind.discover(&DiscoverOpts {
                    root: Some(entry.path.clone()),
                    cwd: cwd.map(str::to_string),
                    limit: 15,
                    ..Default::default()
                }) {
                    push_candidate(&mut out, &mut seen, kind, reference);
                }
            }
            continue;
        }
        // auto（或无法识别的 token）：通用扫描 + 按路径形状分类
        let files: Vec<String> = if meta.is_file() {
            vec![entry.path.clone()]
        } else {
            let mut found = Vec::new();
            scan_jsonl(&entry.path, 2, &mut found);
            found
        };
        for file in files {
            let Some(kind) = adapter_for_file(&file, &entry.agent) else {
                continue;
            };
            if let Some(reference) = file_ref(kind, &file, Some(&entry.agent)) {
                push_candidate(&mut out, &mut seen, kind, reference);
            }
        }
    }
    out
}

fn push_candidate(
    out: &mut Vec<(AdapterKind, SessionRef)>,
    seen: &mut Vec<String>,
    kind: AdapterKind,
    reference: SessionRef,
) {
    let key = format!(
        "{}#{}",
        reference.file_path,
        reference.session_id.clone().unwrap_or_default()
    );
    if seen.contains(&key) {
        return;
    }
    seen.push(key);
    out.push((kind, reference));
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextSummary {
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub basis: Option<String>,
    pub turns_included: usize,
    pub error: Option<String>,
}

/// 会话浏览器：设置里列了什么就扫什么，新→旧
pub fn browse(settings: &AppSettings, limit: usize) -> Vec<SessionRef> {
    let mut all = user_candidates(settings, None)
        .into_iter()
        .map(|(_, r)| r)
        .collect::<Vec<_>>();
    all.sort_by(|a, b| b.mtime_ms.total_cmp(&a.mtime_ms));
    all.truncate(limit);
    all
}

#[derive(Clone, Debug, PartialEq)]
pub struct Payload {
    pub prompt: String,
    pub context: String,
    pub used_chars: usize,
    pub dropped: Vec<String>,
}

/// 面板持有的那「一包」：选区 + 可选的会话上下文
#[derive(Clone, Debug, Default)]
pub struct Pack {
    pub selection: Option<crate::capture::Selection>,
    pub capture_at: String,
    pub transcript: Vec<TranscriptTurn>,
    pub defaults: Vec<String>,
    pub source: Option<Source>,
    pub context: ContextSummary,
}

impl Pack {
    pub fn set_selection(&mut self, selection: crate::capture::Selection) {
        self.capture_at = selection.at.clone();
        self.selection = Some(selection);
        self.transcript.clear();
        self.defaults.clear();
        self.source = None;
        self.context = ContextSummary::default();
    }

    /// 状态行：首行 + 「来源：剪贴板 · 时间 · N 字」；没有选区时给默认文案
    pub fn status_line(&self) -> (String, String) {
        let selection = self.selection.as_ref();
        (
            crate::capture::status_text(selection),
            crate::capture::status_tip(selection),
        )
    }

    /// 判定会话行：没填充就说没填充，出错要说原因，成功要说 agent + 轮数 + 判据
    pub fn session_line(&self, read: bool) -> (String, String, bool) {
        if !read {
            return (String::new(), String::new(), false);
        }
        let context = &self.context;
        if context.agent.is_none() && context.error.is_none() {
            return (
                crate::capture::CONTEXT_EMPTY.to_string(),
                String::new(),
                false,
            );
        }
        if let Some(error) = context.error.as_deref() {
            return (format!("上下文：{error}"), String::new(), true);
        }
        (
            format!(
                "判定会话：{} · {} 轮",
                context.agent.clone().unwrap_or_default(),
                context.turns_included
            ),
            format!(
                "会话 id：{}\n判据：{}",
                context
                    .session_id
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
                context.basis.clone().unwrap_or_default()
            ),
            false,
        )
    }

    pub fn clear_context(&mut self) {
        if self.selection.is_none() {
            return;
        }
        self.transcript.clear();
        // adapter 丢过什么也必须跟着上下文一起清掉，否则直通时字数行会挂着一条不相干的「已省略」
        self.defaults.clear();
        self.source = None;
        self.context = ContextSummary::default();
    }

    /// 显式触发才会跑（可能要几百毫秒）：发现 → 判定会话 → 读 transcript
    pub fn attach(
        &mut self,
        settings: &AppSettings,
        agent: &str,
        turns: usize,
        file_path: Option<&str>,
        session_id: Option<&str>,
    ) {
        if self.selection.is_none() {
            self.context = ContextSummary {
                error: Some("请先取入选区".into()),
                ..Default::default()
            };
            return;
        }
        let cwd_hint = settings
            .session_paths
            .iter()
            .find(|p| p.agent == "project")
            .map(|p| p.path.clone());

        let chosen: Option<(AdapterKind, SessionRef, String)> = if let Some(file) =
            file_path.or(cwd_hint.as_deref().filter(|p| p.ends_with(".jsonl")))
        {
            if !std::path::Path::new(file).exists() {
                self.context = ContextSummary {
                    error: Some(format!("会话文件不存在：{file}")),
                    ..Default::default()
                };
                return;
            }
            let Some(kind) = adapter_for_file(file, agent) else {
                self.context = ContextSummary {
                    error: Some(format!("无法判定该文件的 agent 类型：{file}")),
                    ..Default::default()
                };
                return;
            };
            let mut reference = file_ref(kind, file, None).unwrap_or_else(|| SessionRef {
                agent: kind.agent().to_string(),
                adapter: kind.id().to_string(),
                file_path: file.to_string(),
                ..Default::default()
            });
            let base = base_name(file);
            reference.session_id = session_id
                .map(str::to_string)
                .or_else(|| util::uuid_like(&base));
            let basis = if file_path.is_some() {
                "用户手动选择".to_string()
            } else {
                "设置指定会话文件".to_string()
            };
            Some((kind, reference, basis))
        } else {
            let mut candidates = user_candidates(settings, cwd_hint.as_deref());
            if agent != "auto" {
                candidates.retain(|(kind, r)| r.agent == agent || kind.agent() == agent);
            }
            candidates.sort_by(|a, b| b.1.mtime_ms.total_cmp(&a.1.mtime_ms));
            let refs: Vec<SessionRef> = candidates.iter().map(|(_, r)| r.clone()).collect();
            pick_session(&refs, cwd_hint.as_deref()).and_then(|(reference, basis)| {
                let kind = candidates
                    .iter()
                    .find(|(_, r)| r.file_path == reference.file_path)
                    .map(|(kind, _)| *kind)?;
                Some((kind, reference, basis))
            })
        };

        let Some((kind, reference, basis)) = chosen else {
            self.context = ContextSummary {
                error: Some("未发现可用会话文件，将只带选区".to_string()),
                ..Default::default()
            };
            return;
        };

        let result = kind.read(&reference);
        // 一轮 = 一次用户 + 一次助手；turns <= 0 表示「全部」，不截取
        let slice: Vec<TranscriptTurn> = if turns > 0 {
            let keep = turns.saturating_mul(2);
            if result.turns.len() > keep {
                result.turns[result.turns.len() - keep..].to_vec()
            } else {
                result.turns.clone()
            }
        } else {
            result.turns.clone()
        };
        let mut defaults = result.dropped.clone();
        if let Some(error) = &result.error {
            defaults.insert(0, format!("adapter-error:{error}"));
        }
        self.transcript = slice;
        self.defaults = defaults;
        self.source = Some(Source {
            agent: Some(reference.agent.clone()),
            app: Some("desktop".into()),
            session_id: reference.session_id.clone(),
            project_path: reference.project_path.clone().or(cwd_hint),
            adapter: Some(reference.adapter.clone()),
        });
        self.context = ContextSummary {
            agent: Some(reference.agent.clone()),
            session_id: reference.session_id.clone(),
            basis: Some(if result.error.is_some() {
                format!("{basis}（解析失败，降级为只带选区）")
            } else {
                basis
            }),
            turns_included: if result.error.is_some() {
                0
            } else {
                self.transcript.len().div_ceil(2)
            },
            error: result.error.clone(),
        };
    }

    /// 复制出去的内容：没有上下文就是选区原文本身（逐字节），有上下文才套指令组装
    pub fn payload(&self, settings: &AppSettings) -> Option<Payload> {
        let selection_text = self.selection.as_ref()?.text.clone();
        let capture = Capture {
            via: "clipboard".into(),
            at: self.capture_at.clone(),
        };
        let base = BuildInput::new(
            Selection {
                text: selection_text.clone(),
                role: None,
                anchor: None,
            },
            capture,
        );
        let base = BuildInput {
            source: self.source.clone(),
            transcript: self.transcript.clone(),
            max_chars: NO_BUDGET,
            template_id: TEMPLATE.into(),
            default_dropped: self.defaults.clone(),
            ..base
        };
        let mut pack = build_pack(&base).ok()?;
        let has_context = !self.transcript.is_empty();
        let mut dropped = pack
            .limits
            .clone()
            .and_then(|l| l.dropped)
            .unwrap_or_default();
        // 与 capture.ts 一样：脱敏后的那一份才是组装的输入
        if settings.redact_paths {
            pack = redact_paths(&pack, &home_dir());
            pack.payload = Some(render(&pack, TEMPLATE, &dropped));
        }
        let context = if has_context {
            pack.payload.clone().unwrap_or_default()
        } else {
            String::new()
        };

        if context.is_empty() {
            // 直通：脱敏只服务于上下文组装，绝不碰用户自己复制的选区
            return Some(Payload {
                prompt: selection_text.clone(),
                context: String::new(),
                used_chars: utf16::len(&selection_text),
                dropped,
            });
        }

        let assembled = ctxpack::prompt::assemble_prompt(&AssembleInput {
            template: &active_template(settings),
            selection: &pack.selection.clone().map(|s| s.text).unwrap_or_default(),
            context: &context,
            max_chars: NO_BUDGET,
        });
        dropped.extend(assembled.dropped);
        Some(Payload {
            prompt: assembled.prompt.clone(),
            context,
            used_chars: utf16::len(&assembled.prompt),
            dropped,
        })
    }
}

impl Payload {
    /// 字数行：省略必须看得见，明细放 tooltip
    pub fn meta(&self) -> (String, String) {
        let noun = if self.context.is_empty() {
            "选区原文"
        } else {
            "组装后"
        };
        if self.dropped.is_empty() {
            (format!("{noun} {} 字", self.used_chars), String::new())
        } else {
            (
                format!(
                    "{noun} {} 字 · 已省略 {} 类",
                    self.used_chars,
                    self.dropped.len()
                ),
                format!("已省略：{}", self.dropped.join("、")),
            )
        }
    }
}

fn home_dir() -> String {
    dirs::home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use settings::{PromptTemplate, SessionPath};
    use test_support::fixture_home;

    fn settings_for(home: &str) -> AppSettings {
        AppSettings {
            session_paths: vec![
                SessionPath {
                    agent: "claude-code".into(),
                    path: format!("{home}/.claude/projects"),
                },
                SessionPath {
                    agent: "codex".into(),
                    path: format!("{home}/.codex/sessions"),
                },
                SessionPath {
                    agent: "workbuddy".into(),
                    path: format!("{home}/.workbuddy/projects"),
                },
                SessionPath {
                    agent: "qoder".into(),
                    path: format!("{home}/.qoder-cn/projects"),
                },
                SessionPath {
                    agent: "qoder".into(),
                    path: format!("{home}/Library/Application Support/QoderWork/data/agents.db"),
                },
                SessionPath {
                    agent: "project".into(),
                    path: format!("{home}/Dev/prj"),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn adapter_for_file_classifies_by_path_shape() {
        assert_eq!(
            adapter_for_file("/x/.codex/sessions/a.jsonl", "auto"),
            Some(AdapterKind::Codex)
        );
        assert_eq!(
            adapter_for_file("/x/.workbuddy/projects/a.jsonl", "auto"),
            Some(AdapterKind::Workbuddy)
        );
        assert_eq!(
            adapter_for_file("/x/.qoder-cn/projects/a.jsonl", "auto"),
            Some(AdapterKind::QoderCn)
        );
        assert_eq!(
            adapter_for_file("/x/QoderWork/data/agents.db", "auto"),
            Some(AdapterKind::QoderWork)
        );
        assert_eq!(
            adapter_for_file("/x/plain/a.jsonl", "auto"),
            Some(AdapterKind::ClaudeCode)
        );
        assert_eq!(adapter_for_file("/x/plain/a.txt", "auto"), None);
        assert_eq!(
            adapter_for_file("/x/whatever", "codex"),
            Some(AdapterKind::Codex),
            "具名 hint 优先"
        );
    }

    #[test]
    fn browse_lists_every_configured_source_newest_first() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let refs = browse(&settings_for(&root), BROWSE_LIMIT);
        let agents: Vec<&str> = refs.iter().map(|r| r.agent.as_str()).collect();
        for wanted in ["claude-code", "codex", "workbuddy", "qoder"] {
            assert!(agents.contains(&wanted), "{agents:?}");
        }
        // 新→旧
        assert!(refs.windows(2).all(|w| w[0].mtime_ms >= w[1].mtime_ms));
        // project 条目只是 cwd 提示，不该被当成会话源
        assert!(refs.iter().all(|r| r.adapter != "project"));
    }

    #[test]
    fn candidates_dedupe_by_file_and_session() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let mut settings = settings_for(&root);
        settings.session_paths.push(SessionPath {
            agent: "codex".into(),
            path: format!("{root}/.codex/sessions"),
        });
        let candidates = user_candidates(&settings, None);
        let keys: Vec<String> = candidates
            .iter()
            .map(|(_, r)| {
                format!(
                    "{}#{}",
                    r.file_path,
                    r.session_id.clone().unwrap_or_default()
                )
            })
            .collect();
        assert_eq!(
            keys.len(),
            keys.iter().collect::<std::collections::HashSet<_>>().len(),
            "重复条目要合并"
        );
    }

    /// 测试里造选区：走一遍真实的 from_clipboard，字段口径与 UI 取入时一致
    fn sel(text: &str, at: &str) -> crate::capture::Selection {
        crate::capture::from_clipboard(text, at).unwrap()
    }

    #[test]
    fn attach_with_an_explicit_file_reads_that_session() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let settings = settings_for(&root);
        let file =
            format!("{root}/.codex/sessions/2026/09/11/rollout-2026-09-11T10-15-33-cx-1.jsonl");
        let mut pack = Pack::default();
        pack.set_selection(sel("选中的词", "2026-09-24T10:12:01Z"));
        pack.attach(&settings, "auto", 8, Some(&file), None);
        assert_eq!(pack.context.error, None, "{:?}", pack.context);
        assert_eq!(pack.context.agent.as_deref(), Some("codex"));
        assert_eq!(pack.context.basis.as_deref(), Some("用户手动选择"));
        assert_eq!(pack.context.turns_included, 1);
        assert_eq!(pack.transcript.len(), 2);
    }

    #[test]
    fn attach_without_anything_selectable_reports_a_reason() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let mut pack = Pack::default();
        pack.attach(&settings_for(&root), "auto", 8, None, None);
        assert_eq!(pack.context.error.as_deref(), Some("请先取入选区"));

        pack.set_selection(sel("词", "t"));
        let mut empty = settings_for(&root);
        empty.session_paths.clear();
        pack.attach(&empty, "auto", 8, None, None);
        assert_eq!(
            pack.context.error.as_deref(),
            Some("未发现可用会话文件，将只带选区")
        );
    }

    /// 切到直通：上下文清掉之后，payload 必须立刻回到「选区原文逐字节」，
    /// 而且不能留着上一轮 adapter 的省略记录（app.rs 的复制按钮和字数行都读它）。
    #[test]
    fn clearing_the_context_falls_back_to_the_bare_selection() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let settings = settings_for(&root);
        let file =
            format!("{root}/.codex/sessions/2026/09/11/rollout-2026-09-11T10-15-33-cx-1.jsonl");
        let mut pack = Pack::default();
        pack.set_selection(sel("这个 skill", "t"));
        pack.attach(&settings, "auto", 8, Some(&file), None);
        assert!(!pack.transcript.is_empty());
        pack.clear_context();
        let payload = pack.payload(&settings).unwrap();
        assert_eq!(payload.prompt, "这个 skill", "上下文清完还是逐字节原文");
        assert!(payload.context.is_empty());
        assert_eq!(
            payload.dropped,
            Vec::<String>::new(),
            "上一轮的省略记录不该跟着"
        );
        assert_eq!(payload.meta(), ("选区原文 8 字".to_string(), String::new()));
    }

    #[test]
    fn payload_without_context_is_the_selection_verbatim() {
        let settings = AppSettings::default();
        let mut pack = Pack::default();
        pack.set_selection(sel("原文 with spaces\n第二行", "t"));
        let payload = pack.payload(&settings).unwrap();
        assert_eq!(payload.prompt, "原文 with spaces\n第二行", "直通逐字节原样");
        assert_eq!(payload.context, "");
        assert!(payload.dropped.is_empty());
        assert_eq!(payload.used_chars, utf16::len("原文 with spaces\n第二行"));
    }

    #[test]
    fn payload_with_context_wraps_the_selection_in_the_instruction() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let settings = settings_for(&root);
        let file =
            format!("{root}/.codex/sessions/2026/09/11/rollout-2026-09-11T10-15-33-cx-1.jsonl");
        let mut pack = Pack::default();
        pack.set_selection(sel("这个 skill", "2026-09-24T10:12:01Z"));
        pack.attach(&settings, "auto", 8, Some(&file), None);
        let payload = pack.payload(&settings).unwrap();
        assert!(
            payload.prompt.starts_with(
                "请根据以下用户与agent的交互记录，解释用户选中的词 / 句子：「这个 skill」"
            ),
            "{}",
            payload.prompt
        );
        assert!(
            payload.prompt.ends_with(
                "\n\n用户> 这个skill会token用量过高吗？\n\n助手> 会有，主要瓶颈在检索轮数。"
            ),
            "{}",
            payload.prompt
        );
        assert!(payload.context.contains("用户> "));
        assert_eq!(payload.used_chars, utf16::len(&payload.prompt));
    }

    #[test]
    fn redact_touches_the_assembled_context_but_never_the_raw_selection() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let mut settings = settings_for(&root);
        settings.redact_paths = true;
        let file =
            format!("{root}/.codex/sessions/2026/09/11/rollout-2026-09-11T10-15-33-cx-1.jsonl");
        // 脱敏针对的是真实家目录，所以选区里放的是本机 home 而不是 fixture 目录
        let real_home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        let selection = format!("看 {real_home}/secret/a.ts");

        let mut pack = Pack::default();
        pack.set_selection(sel(&selection, "t"));
        pack.attach(&settings, "auto", 8, Some(&file), None);
        let payload = pack.payload(&settings).unwrap();
        assert!(
            payload.prompt.contains("~/secret/a.ts"),
            "{}",
            payload.prompt
        );
        assert!(!payload.prompt.contains(&real_home));

        // 无上下文（直通）：原样，一个字节都不改
        let mut bare = Pack::default();
        bare.set_selection(sel(&selection, "t"));
        assert_eq!(bare.payload(&settings).unwrap().prompt, selection);
    }

    #[test]
    fn active_prompt_is_the_one_used_for_assembling() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let mut settings = settings_for(&root);
        settings.prompts.push(PromptTemplate {
            name: "第二个".into(),
            template: "第二指令：{selection}".into(),
        });
        settings.active_prompt = 1;
        let file =
            format!("{root}/.codex/sessions/2026/09/11/rollout-2026-09-11T10-15-33-cx-1.jsonl");
        let mut pack = Pack::default();
        pack.set_selection(sel("词", "t"));
        pack.attach(&settings, "auto", 8, Some(&file), None);
        let prompt = pack.payload(&settings).unwrap().prompt;
        assert!(prompt.starts_with("第二指令：词"), "{prompt}");
        assert!(
            prompt.ends_with("助手> 会有，主要瓶颈在检索轮数。"),
            "{prompt}"
        );
    }

    #[test]
    fn turns_zero_keeps_the_whole_transcript() {
        let home = fixture_home();
        let root = home.home.to_string_lossy().to_string();
        let settings = settings_for(&root);
        let file = format!(
            "{root}/.claude/projects/-Users-x-Dev-prj/11111111-2222-3333-4444-555555555555.jsonl"
        );
        let mut pack = Pack::default();
        pack.set_selection(sel("词", "t"));
        pack.attach(&settings, "claude-code", 0, Some(&file), None);
        let all = pack.transcript.len();
        assert!(all >= 2, "{all}");
        pack.attach(&settings, "claude-code", 1, Some(&file), None);
        assert_eq!(pack.transcript.len(), 2, "1 轮 = 一问一答");
        assert!(pack.defaults.iter().any(|d| d == "assistant-thinking"));
    }
}
