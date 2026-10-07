//! 与 Electron 版共用同一个 `~/Library/Application Support/select-assist/settings.json`。
//! 原生版目前只拥有窗口位置，所以读-改-写时其余键必须原样保留（两版都会 patch 这个文件）。

use std::fs;
use std::io::{self, ErrorKind};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Map, Value};

pub const WINDOW_X: &str = "windowX";
pub const WINDOW_Y: &str = "windowY";

/// 站点按钮，字段名与 Electron 的 SiteTarget 一致
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SiteTarget {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Settings {
    file: PathBuf,
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

    /// 窗口左上角的全局坐标（与 Electron `getBounds()` 同一约定）
    pub fn position(&self) -> Option<(f64, f64)> {
        let map = self.read().ok()?;
        Some((map.get(WINDOW_X)?.as_f64()?, map.get(WINDOW_Y)?.as_f64()?))
    }

    /// 直通模式的目标站。没配就是空列表——原生版不另存一份默认值，
    /// 默认站点属于 Electron 那边的 SettingsStore，两版共用同一个文件。
    pub fn direct_sites(&self) -> Vec<SiteTarget> {
        let Ok(map) = self.read() else {
            return Vec::new();
        };
        match map.get("directSites") {
            Some(value) => serde_json::from_value(value.clone()).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    pub fn patch_position(&self, x: f64, y: f64) -> io::Result<()> {
        // 解析失败就直接冒泡：宁可丢掉一次位置，也不能拿半截数据覆盖用户的设置
        let mut map = self.read()?;
        map.insert(WINDOW_X.into(), Value::from(x.round() as i64));
        map.insert(WINDOW_Y.into(), Value::from(y.round() as i64));

        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        // 另一版随时可能读到它，所以先写临时文件再原子替换
        let tmp = self.file.with_extension("json.tmp-native");
        fs::write(&tmp, serde_json::to_vec_pretty(&Value::Object(map))?)?;
        fs::rename(&tmp, &self.file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn reads_direct_sites_and_tolerates_garbage() {
        let file = temp_name("sites");
        let settings = Settings::at(file.clone());
        fs::write(
            &file,
            r#"{"directSites":[{"name":"Google","url":"https://www.google.com/"}]}"#,
        )
        .unwrap();
        assert_eq!(
            settings.direct_sites(),
            vec![SiteTarget {
                name: "Google".into(),
                url: "https://www.google.com/".into()
            }]
        );

        // 条目缺字段 → 整组退回空，而不是把半个站点摆到界面上
        fs::write(&file, r#"{"directSites":[{"name":"缺 url"}]}"#).unwrap();
        assert_eq!(settings.direct_sites(), Vec::new());
        fs::remove_file(&file).ok();
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
