use crate::{Paths, RuntimeStatus};
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
pub const BUS: &str = "org.waytoys.Runtime1";
pub const PATH: &str = "/Waytoys";
pub const INTERFACE: &str = "org.waytoys.Runtime1";
pub fn call(method: &str, value: &str) -> Result<RuntimeStatus, String> {
    let connection = zbus::blocking::connection::Builder::session()
        .map_err(|e| e.to_string())?
        .method_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let proxy =
        zbus::blocking::Proxy::new(&connection, BUS, PATH, INTERFACE).map_err(|e| e.to_string())?;
    let response: String = if method == "Status" {
        proxy.call(method, &()).map_err(|e| e.to_string())?
    } else {
        proxy.call(method, &(value,)).map_err(|e| e.to_string())?
    };
    serde_json::from_str(&response).map_err(|e| e.to_string())
}
pub fn ensure_running(paths: &Paths) -> Result<RuntimeStatus, String> {
    if let Ok(status) = call("Status", "") {
        return check_paths(status, paths);
    }
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("waytoys");
    // Give the runtime its own cgroup. A settings window started by a desktop
    // launcher or transient service must not own the runtime's lifetime.
    let mut command = Command::new("systemd-run");
    command.args([
        "--user",
        "--unit=waytoys-runtime",
        "--collect",
        "--quiet",
        "--service-type=exec",
        "--property=PartOf=graphical-session.target",
    ]);
    for name in [
        "WAYLAND_DISPLAY",
        "XDG_CONFIG_HOME",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ] {
        if let Some(value) = std::env::var_os(name) {
            let mut argument = std::ffi::OsString::from(format!("--setenv={name}="));
            argument.push(value);
            command.arg(argument);
        }
    }
    let mut child = command
        .arg(executable)
        .arg("--background")
        .args(paths.args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Waytoys 실행 실패: {e}"))?;
    let start = Instant::now();
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Waytoys 서비스 시작 시간이 초과되었습니다".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(status) = call("Status", "") {
            return check_paths(status, paths);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if output.status.success() {
        Err("Waytoys 런타임 연결 시간이 초과되었습니다".into())
    } else {
        Err(format!(
            "Waytoys 서비스 시작 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
pub fn check_paths(status: RuntimeStatus, paths: &Paths) -> Result<RuntimeStatus, String> {
    if status.paths != *paths {
        return Err(format!(
            "다른 설정 경로의 Waytoys가 실행 중입니다: {}",
            status.paths.app.display()
        ));
    }
    Ok(status)
}
pub fn settings_control(method: &str, value: &str) -> Result<bool, String> {
    let connection = zbus::blocking::connection::Builder::session()
        .map_err(|e| e.to_string())?
        .method_timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    let bus = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(|e| e.to_string())?;
    let exists: bool = bus
        .call("NameHasOwner", &("org.waytoys.Settings1",))
        .map_err(|e| e.to_string())?;
    if !exists {
        return Ok(method == "Close");
    }
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.waytoys.Settings1",
        "/Settings",
        "org.waytoys.Settings1",
    )
    .map_err(|e| e.to_string())?;
    proxy.call(method, &(value,)).map_err(|e| e.to_string())
}
