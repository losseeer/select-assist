//! 与 packages/ctxpack/test/adapters.test.mjs 一一对应（14 例）。
//! fixture 是从 TS 的 fixtures.mjs 真实生成后复制到 native/fixtures/ 的，agents.db 在测试里按同样结构重建。

use test_support::{fixture_home, scratch_dir, write_qoder_work_db, FixtureHome};

use crate::adapters::claude_code;
use crate::adapters::codex::Codex;
use crate::adapters::qoder::Qoder;
use crate::adapters::util::match_cwd;
use crate::adapters::workbuddy::Workbuddy;
use crate::adapters::{Adapter, DiscoverOpts, SessionRef};
use crate::pick_session;

fn opts(home: &FixtureHome) -> DiscoverOpts {
    DiscoverOpts {
        cwd: Some(home.cwd.clone()),
        home: Some(home.home.to_string_lossy().to_string()),
        ..Default::default()
    }
}

fn roles(turns: &[crate::types::TranscriptTurn]) -> Vec<String> {
    turns.iter().map(|t| t.role.clone()).collect()
}

fn pairs(turns: &[crate::types::TranscriptTurn]) -> Vec<(String, String)> {
    turns
        .iter()
        .map(|t| (t.role.clone(), t.text.clone()))
        .collect()
}

#[test]
fn claude_code_discovery_matches_cwd_and_reports_basis() {
    let home = fixture_home();
    let refs = claude_code::adapter().discover(&opts(&home));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].session_id.as_ref().unwrap().len(), 36);
    assert_eq!(refs[0].project_path.as_deref(), Some(home.cwd.as_str()));
    let (_, basis) = pick_session(&refs, Some(&home.cwd)).unwrap();
    assert!(basis.contains("精确匹配"), "{basis}");
}

#[test]
fn claude_code_transcript_keeps_user_assistant_text_only() {
    let home = fixture_home();
    let adapter = claude_code::adapter();
    let refs = adapter.discover(&opts(&home));
    let result = adapter.read_transcript(&refs[0]);
    assert_eq!(result.error, None);
    let blob = format!("{:?}", result.turns);
    assert!(!blob.contains("SUBAGENT"), "sidechain 要排除");
    assert!(!blob.contains("secret internal"), "thinking 要排除");
    assert!(!blob.contains("huge file"), "tool_result 要排除");
    assert!(!blob.contains("SUMMARY MUST NOT APPEAR"), "续接摘要要排除");
    assert_eq!(roles(&result.turns), ["user", "assistant"]);
    assert!(
        result.turns[0].text.contains("redis预扣库存"),
        "合成 IDE 标签要脱掉、真文本留下"
    );
    assert!(result.turns[1].text.contains("可以用超时机制回滚"));
    assert!(result.turns.last().unwrap().text.contains("waterfall"));
    for expected in [
        "tool-results",
        "assistant-thinking",
        "tool-calls",
        "sidechain-turns",
        "compact-summary",
    ] {
        assert!(
            result.dropped.contains(&expected.to_string()),
            "dropped 应含 {expected}：{:?}",
            result.dropped
        );
    }
}

#[test]
fn codex_discovery_reads_session_meta_cwd() {
    let home = fixture_home();
    let refs = Codex.discover(&opts(&home));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].session_id.as_deref(), Some("cx-1"));
    assert_eq!(refs[0].project_path.as_deref(), Some(home.cwd.as_str()));
}

#[test]
fn codex_transcript_excludes_developer_reasoning_and_tools() {
    let home = fixture_home();
    let refs = Codex.discover(&opts(&home));
    let result = Codex.read_transcript(&refs[0]);
    assert_eq!(result.error, None);
    let blob = format!("{:?}", result.turns);
    assert!(!blob.contains("SYSTEM PROMPT"));
    assert!(!blob.contains("INTERNAL REASONING"));
    assert!(!blob.contains("MUST NOT APPEAR"));
    assert_eq!(
        pairs(&result.turns),
        vec![
            (
                "user".to_string(),
                "这个skill会token用量过高吗？".to_string()
            ),
            (
                "assistant".to_string(),
                "会有，主要瓶颈在检索轮数。".to_string()
            ),
        ]
    );
    assert!(result.dropped.contains(&"tool-results".to_string()));
    assert!(result.dropped.contains(&"reasoning".to_string()));
}

#[test]
fn pick_session_falls_back_to_mtime_guess_and_says_so() {
    let home = fixture_home();
    let adapter = claude_code::adapter();
    let refs = adapter.discover(&DiscoverOpts {
        home: Some(home.home.to_string_lossy().to_string()),
        ..Default::default()
    });
    let (picked, basis) = pick_session(&refs, Some("/no/such/dir")).unwrap();
    assert_eq!(picked.file_path, refs[0].file_path);
    assert!(basis.contains("猜测"), "{basis}");
}

#[test]
fn workbuddy_discovery_with_cwd_match_and_preview() {
    let home = fixture_home();
    let refs = Workbuddy.discover(&opts(&home));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].project_path.as_deref(), Some(home.cwd.as_str()));
    assert!(
        refs[0]
            .preview
            .as_deref()
            .unwrap()
            .contains("这个 JSON 够不够"),
        "user_query 标签要脱掉"
    );
}

#[test]
fn workbuddy_transcript_keeps_message_text_only() {
    let home = fixture_home();
    let refs = Workbuddy.discover(&opts(&home));
    let result = Workbuddy.read_transcript(&refs[0]);
    assert_eq!(result.error, None);
    let blob = format!("{:?}", result.turns);
    assert!(!blob.contains("MUST NOT APPEAR"));
    assert_eq!(roles(&result.turns), ["user", "assistant"]);
    assert!(result.turns[0].text.contains("JSON"));
    assert!(result.dropped.contains(&"tool-results".to_string()));
    assert!(result.dropped.contains(&"reasoning".to_string()));
}

fn qoder_work_refs(all: &[SessionRef]) -> Vec<SessionRef> {
    all.iter()
        .filter(|r| r.adapter.starts_with("qoderwork"))
        .cloned()
        .collect()
}

#[test]
fn qoder_sqlite_discovery_and_transcript() {
    let home = fixture_home();
    let all = Qoder.discover(&opts(&home));
    let refs = qoder_work_refs(&all);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name.as_deref(), Some("面试准备"));
    assert_eq!(refs[0].project_path.as_deref(), Some(home.cwd.as_str()));
    let result = Qoder.read_transcript(&refs[0]);
    assert_eq!(result.error, None);
    let blob = format!("{:?}", result.turns);
    assert!(!blob.contains("PRIVATE"), "非文本 part 要排除");
    assert!(!blob.contains("MUST NOT APPEAR"), "error part 要排除");
    assert_eq!(
        pairs(&result.turns),
        vec![
            ("user".to_string(), "围绕项目向我提问".to_string()),
            ("assistant".to_string(), "好的，第一个问题：".to_string()),
        ]
    );
    assert!(result.dropped.contains(&"tool-parts".to_string()));
    assert!(result.dropped.contains(&"errors".to_string()));
}

#[test]
fn qoder_cn_ide_jsonl_discovery_and_transcript() {
    let home = fixture_home();
    let all = Qoder.discover(&opts(&home));
    let refs: Vec<SessionRef> = all
        .iter()
        .filter(|r| r.adapter.starts_with("qoder-cn"))
        .cloned()
        .collect();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].project_path.as_deref(), Some(home.cwd.as_str()));
    assert!(refs[0].preview.as_deref().unwrap().contains("悬浮窗"));
    let result = Qoder.read_transcript(&refs[0]);
    assert_eq!(result.error, None);
    let blob = format!("{:?}", result.turns);
    assert!(!blob.contains("MUST NOT APPEAR"));
    assert!(!blob.contains("PRIVATE"));
    assert_eq!(
        pairs(&result.turns),
        vec![
            ("user".to_string(), "这个悬浮窗怎么不抢焦点？".to_string()),
            (
                "assistant".to_string(),
                "把 chip 窗设为 focusable:false 就常驻不抢焦点。".to_string()
            ),
        ]
    );
    assert!(result.dropped.contains(&"assistant-thinking".to_string()));
    assert!(result.dropped.contains(&"tool-calls".to_string()));
}

#[test]
fn corrupt_file_degrades_with_error_not_throw() {
    let home = fixture_home();
    // 逐段 join，和 adapter 自己的 home_relative 一样：一次塞进 "/" 在 Windows 上会拼出
    // 混合分隔符，比较 file_path 时就成了假失败
    let bad = home
        .home
        .join(".claude")
        .join("projects")
        .join("-bad")
        .join("deadbeef.jsonl");
    std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
    std::fs::write(&bad, "not json\nat all\n").unwrap();
    let adapter = claude_code::adapter();
    let refs = adapter.discover(&DiscoverOpts {
        home: Some(home.home.to_string_lossy().to_string()),
        ..Default::default()
    });
    let ref_ = refs
        .iter()
        .find(|r| r.file_path == bad.to_string_lossy())
        .expect("坏文件也要被发现");
    let result = adapter.read_transcript(ref_);
    assert!(result.error.is_some(), "要给出原因");
    assert!(result.turns.is_empty());
}

#[test]
fn match_cwd_folds_drive_case_and_separators_but_never_posix_case() {
    let stored = "d:\\Projects\\prj";
    assert_eq!(
        match_cwd(Some(stored), Some("D:\\Projects\\prj")),
        Some("exact")
    );
    assert_eq!(
        match_cwd(Some(stored), Some("D:/Projects/prj/")),
        Some("exact")
    );
    assert_eq!(
        match_cwd(Some("D:\\Projects\\prj"), Some(stored)),
        Some("exact")
    );
    assert_eq!(
        match_cwd(Some("D:\\Projects\\prj\\sub"), Some("D:\\Projects\\prj")),
        Some("under")
    );
    assert_eq!(match_cwd(Some("d:\\projects"), Some("d:\\")), Some("under"));
    assert_eq!(
        match_cwd(Some(stored), Some("D:\\Projects\\prj\\sub")),
        None
    );
    assert_eq!(match_cwd(Some("/Users/X/prj"), Some("/users/x/prj")), None);
    assert_eq!(
        match_cwd(Some("/Users/x/prj/sub"), Some("/Users/x/prj")),
        Some("under")
    );
    assert_eq!(match_cwd(None, Some("d:\\x")), None);
    assert_eq!(match_cwd(Some("d:\\x"), None), None);
}

#[test]
fn pick_session_uppercase_cwd_still_finds_the_lowercase_logged_session() {
    let refs = vec![
        SessionRef {
            agent: "qoder".into(),
            adapter: "a".into(),
            file_path: "q.jsonl".into(),
            project_path: Some("D:\\Projects\\other".into()),
            mtime_ms: 9.0,
            ..Default::default()
        },
        SessionRef {
            agent: "claude-code".into(),
            adapter: "b".into(),
            file_path: "c.jsonl".into(),
            project_path: Some("d:\\projects\\prj".into()),
            mtime_ms: 8.0,
            ..Default::default()
        },
    ];
    let (picked, basis) = pick_session(&refs, Some("D:\\Projects\\prj\\")).unwrap();
    assert_eq!(picked.agent, "claude-code", "不能退回到更新但无关的那条");
    assert!(basis.contains("精确匹配"), "{basis}");
}

/// TS 侧靠改 process.platform 验 Windows 布局；Rust 里平台常量改不了，
/// 所以分两半验：路径推导（qoder.rs 的 support_dir 单测）+ 按 APPDATA 布局建库后能读出来
#[test]
fn qoder_work_reads_a_windows_layout_database() {
    let app_data = scratch_dir("appdata");
    let win_cwd = "d:\\Projects\\prj";
    write_qoder_work_db(&app_data.join("QoderWork/data"), win_cwd);
    let db = app_data
        .join("QoderWork/data/agents.db")
        .to_string_lossy()
        .to_string();

    let refs = qoder_work_refs(&Qoder.discover(&DiscoverOpts {
        root: Some(db.clone()),
        limit: 5,
        ..Default::default()
    }));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].file_path, db);
    assert_eq!(refs[0].project_path.as_deref(), Some(win_cwd));
    assert_eq!(refs[0].name.as_deref(), Some("面试准备"));
    let result = Qoder.read_transcript(&refs[0]);
    assert_eq!(result.error, None);
    assert_eq!(
        pairs(&result.turns),
        vec![
            ("user".to_string(), "围绕项目向我提问".to_string()),
            ("assistant".to_string(), "好的，第一个问题：".to_string()),
        ]
    );
    std::fs::remove_dir_all(&app_data).ok();
}

#[test]
fn root_override_scans_the_configured_location() {
    let home = fixture_home();
    let root = |path: &str| DiscoverOpts {
        root: Some(path.to_string()),
        ..Default::default()
    };

    let codex_refs = Codex.discover(&root(&home.home.join(".codex/sessions").to_string_lossy()));
    assert_eq!(codex_refs.len(), 1, "codex 的 root 仍然递归日期目录");

    let empty = std::env::temp_dir().join(format!("sa-root-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let adapter = claude_code::adapter();
    assert!(
        adapter
            .discover(&DiscoverOpts {
                root: Some(empty.to_string_lossy().to_string()),
                home: Some(home.home.to_string_lossy().to_string()),
                ..Default::default()
            })
            .is_empty(),
        "root 要盖过 home 默认"
    );
    std::fs::remove_dir_all(&empty).ok();

    let cn = Qoder.discover(&root(
        &home.home.join(".qoder-cn/projects").to_string_lossy(),
    ));
    assert!(
        !cn.is_empty() && cn.iter().all(|r| r.adapter.starts_with("qoder-cn")),
        "目录 root = 只有 CN"
    );

    let db = Qoder.discover(&root(
        &home
            .home
            .join("Library/Application Support/QoderWork/data/agents.db")
            .to_string_lossy(),
    ));
    assert!(
        !db.is_empty() && db.iter().all(|r| r.adapter.starts_with("qoderwork")),
        ".db root = 只有 sqlite"
    );
}
