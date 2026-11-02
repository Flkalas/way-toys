use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnabledModules {
    pub zones: bool,
    pub paste: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaytoysSettings {
    pub run_at_login: bool,
    pub enabled: EnabledModules,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paths {
    pub app: PathBuf,
    pub zones: PathBuf,
    pub paste: PathBuf,
}
impl Paths {
    pub fn resolve(app: Option<PathBuf>, zones: Option<PathBuf>) -> Result<Self, String> {
        Self::resolve_with_paste(app, zones, None)
    }
    pub fn resolve_with_paste(
        app: Option<PathBuf>,
        zones: Option<PathBuf>,
        paste: Option<PathBuf>,
    ) -> Result<Self, String> {
        let paste = match paste.or_else(|| app.as_ref().map(|p| p.join("paste"))) {
            Some(p) => p,
            None => {
                let base = match std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
                    Some(p) => PathBuf::from(p),
                    None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME이 필요합니다")?)
                        .join(".local/share"),
                };
                if !base.is_absolute() {
                    return Err("XDG 데이터 경로는 절대 경로여야 합니다".into());
                }
                base.join("waytoys/paste")
            }
        };
        let app = match app {
            Some(p) => p,
            None => {
                let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
                    Some(p) => PathBuf::from(p),
                    None => PathBuf::from(std::env::var_os("HOME").ok_or("HOME이 필요합니다")?)
                        .join(".config"),
                };
                if !base.is_absolute() {
                    return Err("XDG 설정 경로는 절대 경로여야 합니다".into());
                }
                base.join("waytoys")
            }
        };
        let zones = zones.unwrap_or_else(|| app.join("zones"));
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let absolute = |p: PathBuf| {
            let p = if p.is_absolute() { p } else { cwd.join(p) };
            p.canonicalize().unwrap_or(p)
        };
        Ok(Self {
            app: absolute(app),
            zones: absolute(zones),
            paste: absolute(paste),
        })
    }
    pub fn args(&self) -> Vec<std::ffi::OsString> {
        vec![
            "--app-config-dir".into(),
            self.app.as_os_str().into(),
            "--config-dir".into(),
            self.zones.as_os_str().into(),
            "--paste-data-dir".into(),
            self.paste.as_os_str().into(),
        ]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub paths: Paths,
    pub settings: WaytoysSettings,
    pub ready: bool,
    pub blocked: bool,
    pub error: Option<String>,
    pub paste_ready: bool,
    pub paste_error: Option<String>,
}
impl RuntimeStatus {
    pub fn label(&self) -> &'static str {
        if self.blocked {
            "설정 읽기 오류 · 원본 보존"
        } else if !self.settings.enabled.zones {
            "Zones 꺼짐"
        } else if self.error.is_some() {
            "Zones 연결 오류 · 현재 창 동작에는 미적용"
        } else if self.ready {
            "Zones 실행 중 · Shift로 창을 영역에 배치하세요"
        } else {
            "Zones 연결 중…"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::JsonStore;
    #[test]
    fn application_flags_are_separate_and_disabled_by_default() {
        assert_eq!(
            serde_json::to_value(WaytoysSettings::default()).unwrap(),
            serde_json::json!({"run_at_login":false,"enabled":{"zones":false,"paste":false}})
        );
    }
    #[test]
    fn older_settings_preserve_zones_and_development_history_is_isolated() {
        let settings: WaytoysSettings =
            serde_json::from_str(r#"{"run_at_login":true,"enabled":{"zones":true}}"#).unwrap();
        assert!(settings.run_at_login && settings.enabled.zones);
        assert!(!settings.enabled.paste);
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(Some(dir.path().join("app")), None).unwrap();
        assert_eq!(paths.paste, dir.path().join("app/paste"));
        let override_paths =
            Paths::resolve_with_paste(Some(paths.app), None, Some(dir.path().join("history")))
                .unwrap();
        assert_eq!(override_paths.paste, dir.path().join("history"));
    }
    #[test]
    fn application_store_preserves_corruption_and_other_writers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let (mut a, mut value) = JsonStore::<WaytoysSettings>::open(path.clone()).unwrap();
        let (mut b, _) = JsonStore::<WaytoysSettings>::open(path.clone()).unwrap();
        value.enabled.zones = true;
        a.save(&value).unwrap();
        assert!(b.save(&WaytoysSettings::default()).is_err());
        assert_eq!(
            JsonStore::<WaytoysSettings>::open(path.clone()).unwrap().1,
            value
        );
        std::fs::write(&path, b"broken original").unwrap();
        assert!(a.save(&value).is_err());
        assert!(JsonStore::<WaytoysSettings>::open(path.clone()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"broken original");
    }
}
