//! 与 Electron 版共用同一个 `~/Library/Application Support/select-assist/settings.json`。
//! 原生版目前只拥有窗口位置，所以读-改-写时其余键必须原样保留（两版都会 patch 这个文件）。

use std::fs;
use std::io::{self, ErrorKind};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
pub const WINDOW_X: &str = "windowX";
pub const WINDOW_Y: &str = "windowY";

/// 站点按钮，字段名与 Electron 的 SiteTarget 一致
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SiteTarget {
    pub name: String,
    pub url: String,
}

impl SiteTarget {
    fn list(pairs: &[(&str, &str)]) -> Vec<SiteTarget> {
        pairs
            .iter()
            .map(|(name, url)| SiteTarget {
                name: (*name).into(),
                url: (*url).into(),
            })
            .collect()
    }
}

/// 提问指令：名称 + 模板（含 {selection} 占位）
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub name: String,
    pub template: String,
}

/// 会话发现只读这张表：agent ∈ auto/claude-code/codex/workbuddy/qoder/project
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionPath {
    pub agent: String,
    pub path: String,
}

/// 面板与原生版共用的那份配置（Electron 侧的 schema v3）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub with_context: bool,
    pub chat_sites: Vec<SiteTarget>,
    pub direct_sites: Vec<SiteTarget>,
    pub prompts: Vec<PromptTemplate>,
    pub active_prompt: usize,
    pub session_paths: Vec<SessionPath>,
    pub redact_paths: bool,
    pub context_turns: usize,
}

pub const DEFAULT_PROMPT_TEMPLATE: &str =
    "请根据以下用户与agent的交互记录，解释用户选中的词 / 句子：「{selection}」";

/// 与 Electron 的 defaultSessionPaths() 同表：删一行就等于停扫那个源
pub fn default_session_paths(home: &str) -> Vec<SessionPath> {
    let mut paths = vec![
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
    ];
    let db = crate::ctxpack::adapters::qoder::qoder_work_db_path(None)
        .to_string_lossy()
        .to_string();
    paths.push(SessionPath {
        agent: "qoder".into(),
        path: db,
    });
    paths
}

impl Default for AppSettings {
    fn default() -> Self {
        let home = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            with_context: true,
            chat_sites: SiteTarget::list(&[
                ("DeepSeek", "https://chat.deepseek.com/"),
                ("ChatGPT", "https://chatgpt.com/"),
                ("Gemini", "https://gemini.google.com/"),
            ]),
            direct_sites: SiteTarget::list(&[
                ("Google", "https://www.google.com/"),
                ("Bing", "https://www.bing.com/"),
                ("DeepL", "https://www.deepl.com/translator"),
                ("有道", "https://fanyi.youdao.com/#/TextTranslation"),
            ]),
            prompts: vec![PromptTemplate {
                name: "解释选区".into(),
                template: DEFAULT_PROMPT_TEMPLATE.into(),
            }],
            active_prompt: 0,
            session_paths: default_session_paths(&home),
            redact_paths: false,
            context_turns: 8,
        }
    }
}

/// 当前生效的提问指令：越界的 activePrompt 退回第一条（与 settings.ts 的兜底一致）
pub fn active_template(settings: &AppSettings) -> String {
    settings
        .prompts
        .get(settings.active_prompt)
        .or_else(|| settings.prompts.first())
        .map(|p| p.template.clone())
        .unwrap_or_else(|| DEFAULT_PROMPT_TEMPLATE.to_string())
}

#[derive(Clone, Debug)]
pub struct Settings {
    file: PathBuf,
}

/// 键存在且形状对得上才覆盖；对不上就保留默认值，让坏数据只影响它自己那一组
fn take<T: serde::de::DeserializeOwned>(map: &Map<String, Value>, key: &str, into: &mut T) {
    if let Some(value) = map.get(key) {
        if let Ok(parsed) = serde_json::from_value(value.clone()) {
            *into = parsed;
        }
    }
}

impl Settings {
    /// Electron 版用 `app.setName('select-assist')` 把 dev 与打包版统一到这个 userData 目录
    pub fn shared() -> Self {
        let home = dirs::home_dir().expect("no home directory");
        Self {
            file: home
                .join("Library/Application Support/select-assist")
                .join("settings.json"),
        }
    }

    #[cfg(test)]
    pub fn at(file: PathBuf) -> Self {
        Self { file }
    }

    fn read(&self) -> io::Result<Map<String, Value>> {
        let bytes = match fs::read(&self.file) {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Map::new()),
            other => other?,
        };
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
        match value {
            Value::Object(map) => Ok(map),
            _ => Err(io::Error::new(
                ErrorKind::InvalidData,
                "settings.json 不是 JSON 对象",
            )),
        }
    }

    /// 读整份配置：缺键回落到默认值，坏文件等于「用默认值继续跑」。
    /// 逐键解析而不是整份 from_value：Electron 那边多写或改写一个键时，坏的那一组回退默认，
    /// 其余组不能跟着一起丢（这份文件两版共用）。
    pub fn load(&self) -> AppSettings {
        let map = self.read().unwrap_or_default();
        let mut settings = AppSettings::default();
        take(&map, "withContext", &mut settings.with_context);
        take(&map, "chatSites", &mut settings.chat_sites);
        // 直通站点：没配就是空列表，默认站点由 Electron 那份文件自己提供
        take(&map, "directSites", &mut settings.direct_sites);
        take(&map, "prompts", &mut settings.prompts);
        take(&map, "sessionPaths", &mut settings.session_paths);
        take(&map, "redactPaths", &mut settings.redact_paths);
        take(&map, "contextTurns", &mut settings.context_turns);
        take(&map, "activePrompt", &mut settings.active_prompt);
        // settings.ts:113 —— 越界游标回到第一条，不是最后一条
        if settings.active_prompt >= settings.prompts.len() {
            settings.active_prompt = 0;
        }
        settings
    }

    /// 整份写回，但只覆盖我们认识的键：maxChars / templateId 这类历史键必须原样留着
    pub fn save(&self, settings: &AppSettings) -> io::Result<()> {
        let mut map = self.read()?;
        let typed = serde_json::to_value(settings)
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
        if let Some(typed) = typed.as_object() {
            for (key, value) in typed {
                map.insert(key.clone(), value.clone());
            }
        }
        self.write_map(map)
    }

    /// 窗口左上角的全局坐标（与 Electron `getBounds()` 同一约定）
    pub fn position(&self) -> Option<(f64, f64)> {
        let map = self.read().ok()?;
        Some((map.get(WINDOW_X)?.as_f64()?, map.get(WINDOW_Y)?.as_f64()?))
    }

    pub fn patch_position(&self, x: f64, y: f64) -> io::Result<()> {
        // 解析失败就直接冒泡：宁可丢掉一次位置，也不能拿半截数据覆盖用户的设置
        let mut map = self.read()?;
        map.insert(WINDOW_X.into(), Value::from(x.round() as i64));
        map.insert(WINDOW_Y.into(), Value::from(y.round() as i64));

        self.write_map(map)
    }

    /// 另一版随时可能读到它，所以先写临时文件再原子替换
    fn write_map(&self, map: Map<String, Value>) -> io::Result<()> {
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.file.with_extension("json.tmp-native");
        fs::write(&tmp, serde_json::to_vec_pretty(&Value::Object(map))?)?;
        fs::rename(&tmp, &self.file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 每个用例各自一个文件名：cargo test 是并行的，共用路径会互相删文件
    fn temp_name(tag: &str) -> PathBuf {
        let file =
            std::env::temp_dir().join(format!("sa-settings-{}-{tag}.json", std::process::id()));
        fs::remove_file(&file).ok();
        file
    }

    #[test]
    fn patch_keeps_other_keys_and_their_order() {
        let file = temp_name("order");
        let settings = Settings::at(file.clone());
        fs::write(
            &file,
            r#"{"withContext":false,"chatSites":[{"name":"DeepSeek"}],"windowX":10,"windowY":20}"#,
        )
        .unwrap();

        settings.patch_position(300.4, 400.6).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        let written: Map<String, Value> = serde_json::from_str(&text).unwrap();

        assert_eq!(written["windowX"], Value::from(300));
        assert_eq!(written["windowY"], Value::from(401));
        assert_eq!(written["withContext"], Value::from(false));
        assert_eq!(written["chatSites"][0]["name"], Value::from("DeepSeek"));
        // 键序不变：用户的文件不该被我们重排
        assert_eq!(
            written.keys().cloned().collect::<Vec<_>>(),
            vec!["withContext", "chatSites", "windowX", "windowY"]
        );
        fs::remove_file(&file).ok();
    }

    #[test]
    fn corrupt_file_is_never_overwritten() {
        let file = temp_name("corrupt");
        let settings = Settings::at(file.clone());
        fs::write(&file, b"{ not json").unwrap();

        assert!(settings.patch_position(1.0, 2.0).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "{ not json");
        assert_eq!(settings.position(), None);
        fs::remove_file(&file).ok();
    }

    #[test]
    fn a_broken_group_falls_back_alone() {
        let file = temp_name("sites");
        let settings = Settings::at(file.clone());
        fs::write(
            &file,
            r#"{"directSites":[{"name":"Google","url":"https://www.google.com/"}],"contextTurns":20}"#,
        )
        .unwrap();
        let loaded = settings.load();
        assert_eq!(
            loaded.direct_sites,
            vec![SiteTarget {
                name: "Google".into(),
                url: "https://www.google.com/".into()
            }]
        );
        assert_eq!(loaded.context_turns, 20);

        // 条目缺字段 → 整组退回默认，而不是把半个站点摆到界面上，也不能牵连别的键
        fs::write(
            &file,
            r#"{"directSites":[{"name":"缺 url"}],"contextTurns":20}"#,
        )
        .unwrap();
        let after = settings.load();
        assert_eq!(after.direct_sites, AppSettings::default().direct_sites);
        assert_eq!(after.context_turns, 20);
        fs::remove_file(&file).ok();
    }

    #[test]
    fn load_merges_the_file_over_defaults() {
        let file = temp_name("load");
        let settings = Settings::at(file.clone());
        fs::write(
            &file,
            r#"{"withContext":false,"contextTurns":30,"maxChars":600,"templateId":"clean/v1","prompts":[{"name":"A","template":"{selection}"}]}"#,
        )
        .unwrap();
        let loaded = settings.load();
        assert!(!loaded.with_context);
        assert_eq!(loaded.context_turns, 30);
        assert_eq!(loaded.prompts.len(), 1);
        // 缺的键回落默认，不是空列表
        assert_eq!(loaded.direct_sites.len(), 4);
        assert!(!loaded.session_paths.is_empty());

        fs::write(
            &file,
            r#"{"activePrompt":9,"prompts":[{"name":"A","template":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(settings.load().active_prompt, 0, "越界游标退回第一条");

        // 三条指令时越界也必须回到 0：min(len-1) 会偷偷选到最后一条，Electron 不是这么兜的
        fs::write(
            &file,
            r#"{"activePrompt":9,"prompts":[{"name":"A","template":"x"},{"name":"B","template":"y"},{"name":"C","template":"z"}]}"#,
        )
        .unwrap();
        assert_eq!(settings.load().active_prompt, 0);
        // 范围内的游标原样保留
        fs::write(
            &file,
            r#"{"activePrompt":2,"prompts":[{"name":"A","template":"x"},{"name":"B","template":"y"},{"name":"C","template":"z"}]}"#,
        )
        .unwrap();
        assert_eq!(settings.load().active_prompt, 2);
        fs::remove_file(&file).ok();
    }

    #[test]
    fn save_keeps_keys_the_native_side_does_not_model() {
        let file = temp_name("save");
        let settings = Settings::at(file.clone());
        fs::write(
            &file,
            r#"{"maxChars":600,"templateId":"clean/v1","expanded":true,"windowX":10,"windowY":20}"#,
        )
        .unwrap();
        let mut loaded = settings.load();
        loaded.redact_paths = true;
        loaded.context_turns = 16;
        settings.save(&loaded).unwrap();

        let raw: Map<String, Value> =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(raw["maxChars"], json!(600));
        assert_eq!(raw["templateId"], json!("clean/v1"));
        assert_eq!(raw["expanded"], json!(true));
        assert_eq!(raw["windowX"], json!(10));
        assert_eq!(raw["redactPaths"], json!(true));
        assert_eq!(raw["contextTurns"], json!(16));
        // 键序仍然属于用户的文件
        assert_eq!(raw.keys().next().unwrap(), "maxChars");
        fs::remove_file(&file).ok();
    }

    #[test]
    fn active_template_falls_back_to_the_first_prompt() {
        let mut settings = AppSettings::default();
        assert_eq!(active_template(&settings), DEFAULT_PROMPT_TEMPLATE);
        settings.prompts.push(PromptTemplate {
            name: "B".into(),
            template: "第二个 {selection}".into(),
        });
        settings.active_prompt = 1;
        assert_eq!(active_template(&settings), "第二个 {selection}");
        settings.prompts.clear();
        assert_eq!(active_template(&settings), DEFAULT_PROMPT_TEMPLATE);
    }

    #[test]
    fn first_run_creates_the_file() {
        let file = temp_name("firstrun");
        let settings = Settings::at(file.clone());

        assert_eq!(settings.position(), None);
        settings.patch_position(1054.0, 93.0).unwrap();
        assert_eq!(settings.position(), Some((1054.0, 93.0)));
        fs::remove_file(&file).ok();
    }
}
