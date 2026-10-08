//! 测试专用：把 native/fixtures/ 铺成一个临时 home，并按 TS fixtures.mjs 的结构建 agents.db。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::Connection;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 共享 fixture 的位置：native/fixtures/，两个 crate 的测试都从这里取
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .canonicalize()
        .expect("native/fixtures 不存在")
}

pub struct FixtureHome {
    pub home: PathBuf,
    pub cwd: String,
}

impl Drop for FixtureHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// fixture 里的绝对路径写作 __HOME__，复制到临时 home 时代入真实位置
fn copy_tree(from: &Path, to: &Path, home: &str) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target, home);
        } else {
            let text = std::fs::read_to_string(entry.path())
                .unwrap()
                .replace("__HOME__", home);
            std::fs::write(target, text).unwrap();
        }
    }
}

/// 与 fixtures.mjs 的 writeQoderWorkDb 同结构同数据
pub fn write_qoder_work_db(data_dir: &Path, cwd: &str) {
    std::fs::create_dir_all(data_dir).unwrap();
    let db = Connection::open(data_dir.join("agents.db")).unwrap();
    db.execute_batch(
        r#"
        create table projects (id text primary key, name text, path text, created_at integer, updated_at integer);
        create table chats (id text primary key, name text, project_id text, created_at integer, updated_at integer, deleted_at integer);
        create table messages (id text primary key, message_id text, chat_id text, sub_chat_id text, sequence integer, role text, parts text, created_at integer);
        "#,
    )
    .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    db.execute(
        "insert into projects values (?,?,?,?,?)",
        rusqlite::params!["p1", "prj", cwd, now, now],
    )
    .unwrap();
    db.execute(
        "insert into chats values (?,?,?,?,?,?)",
        rusqlite::params![
            "chat1",
            "面试准备",
            "p1",
            now - 100,
            now,
            Option::<i64>::None
        ],
    )
    .unwrap();
    let rows = [
        (
            "m1",
            "mm1",
            1,
            "user",
            r#"[{"type":"text","text":"围绕项目向我提问"}]"#,
        ),
        (
            "m2",
            "mm2",
            2,
            "assistant",
            r#"[{"type":"tool-Thinking","input":{"text":"PRIVATE"}},{"type":"text","text":"好的，第一个问题："}]"#,
        ),
        (
            "m3",
            "mm3",
            3,
            "assistant",
            r#"[{"type":"error","text":"MUST NOT APPEAR"}]"#,
        ),
    ];
    for (id, message_id, sequence, role, parts) in rows {
        db.execute(
            "insert into messages values (?,?,?,?,?,?,?,?)",
            rusqlite::params![id, message_id, "chat1", "s1", sequence, role, parts, now],
        )
        .unwrap();
    }
}

/// 每次调用给一个唯一的临时目录名（cargo test 是并行的）
pub fn scratch_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sa-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ))
}

pub fn fixture_home() -> FixtureHome {
    let home = scratch_dir("ctxpack");
    // fixture 里的 __HOME__ 落在 JSON 字符串内部：Windows 路径的反斜杠会拼成非法 JSON 转义，
    // 整行被 parse_json_lines 当坏行丢掉，cwd/preview 就静默变空。统一换成正斜杠形式，
    // Windows 的路径 API 一样接受；macOS 上这次 replace 是原样返回。
    let home_json = home.to_string_lossy().replace("\\", "/");
    copy_tree(&fixtures_dir(), &home, &home_json);
    let cwd = format!("{home_json}/Dev/prj");
    write_qoder_work_db(
        &home.join("Library/Application Support/QoderWork/data"),
        &cwd,
    );
    FixtureHome { home, cwd }
}
