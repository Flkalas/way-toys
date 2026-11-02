use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use waytoys_core::{Paths, storage::atomic_write};

fn quote(value: &str) -> Result<String, String> {
    if value.chars().any(|c| c.is_control()) {
        return Err("자동 실행 경로에 제어 문자를 사용할 수 없습니다".into());
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    ))
}
fn run(args: &[&str]) -> Result<(), String> {
    let mut child = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("자동 실행 서비스 설정 시간 초과".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().into())
    }
}
pub fn configure(paths: &Paths, enabled: bool) -> Result<(), String> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(p) => p.into(),
        None => std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME is missing")?)
            .join(".config"),
    };
    let dir = base.join("systemd/user");
    let destination = dir.join("waytoys.service");
    if std::fs::symlink_metadata(&destination).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("기존 서비스 심볼릭 링크를 보존했습니다".into());
    }
    match std::fs::read_to_string(&destination) {
        Ok(previous) if !previous.starts_with("# Managed by Waytoys\n") => {
            return Err("기존 waytoys.service를 보존했습니다. 자동 실행 설정을 확인하세요".into());
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        _ => {}
    }
    if enabled {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        atomic_write(&destination, unit(&executable, paths)?.as_bytes())
            .map_err(|e| e.to_string())?;
        run(&["daemon-reload"])?;
        run(&["enable", "waytoys.service"])
    } else if destination.exists() {
        run(&["disable", "waytoys.service"])
    } else {
        Ok(())
    }
}
fn unit(executable: &Path, paths: &Paths) -> Result<String, String> {
    let q = |p: &Path| quote(p.to_str().ok_or("자동 실행 경로는 UTF-8이어야 합니다")?);
    Ok(format!(
        "# Managed by Waytoys\n[Unit]\nDescription=Waytoys desktop tools\nPartOf=graphical-session.target\nAfter=graphical-session-pre.target plasma-kwin_wayland.service\n\n[Service]\nType=simple\nExecStart={} --background --app-config-dir {} --config-dir {}\nRestart=on-failure\nRestartSec=2\n\n[Install]\nWantedBy=graphical-session.target\n",
        q(executable)?,
        q(&paths.app)?,
        q(&paths.zones)?
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unit_escapes_paths_and_keeps_module_state_out_of_service() {
        let p = Paths {
            app: "/tmp/a % $\"b".into(),
            zones: "/tmp/zones".into(),
            paste: "/tmp/paste".into(),
        };
        let text = unit(Path::new("/opt/Way toys/waytoys"), &p).unwrap();
        assert!(text.contains("\"/opt/Way toys/waytoys\""));
        assert!(text.contains("%% $$\\\""));
        assert!(!text.contains("zones-enabled"));
        assert!(quote("bad\npath").is_err());
    }
}
