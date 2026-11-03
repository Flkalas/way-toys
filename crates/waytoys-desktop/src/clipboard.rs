//! Bounded X11 selection requests. xclip implements ICCCM TARGETS and INCR.
use std::{
    error::Error,
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn read_x11(target: &str, limit: usize) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard", "-out", "-target", target])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let result = (|| -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let bytes = rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err("X11 클립보드 항목을 읽지 못했습니다".into());
                }
                return Ok(bytes);
            }
            if Instant::now() >= deadline {
                return Err("X11 클립보드 응답 시간이 초과되었습니다".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let bytes = result?;
    if bytes.len() > limit {
        return Err("클립보드 항목 크기 제한을 초과했습니다".into());
    }
    Ok(bytes)
}
