use crate::image::{Image, MAX_PIXELS};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{Fd, OwnedValue, Value},
};

pub fn screenshot(connection: &Connection, output: &str) -> Result<Image, String> {
    let proxy = Proxy::new(
        connection,
        "org.kde.KWin.ScreenShot2",
        "/org/kde/KWin/ScreenShot2",
        "org.kde.KWin.ScreenShot2",
    )
    .map_err(|e| e.to_string())?;
    let (mut reader, writer) = UnixStream::pair().map_err(|e| e.to_string())?;
    reader
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let options = HashMap::from([
        ("native-resolution", Value::from(true)),
        ("include-cursor", Value::from(false)),
    ]);
    let info: HashMap<String,OwnedValue>=proxy.call("CaptureScreen",&(output,options,Fd::from(&writer))).map_err(|e|format!("화면을 가져오지 못했습니다: {e}. scripts/install-capture.sh 설치와 화면 잠금을 확인하세요"))?;
    drop(writer);
    let number = |name: &str| -> Result<u32, String> {
        u32::try_from(
            info.get(name)
                .ok_or_else(|| format!("누락된 이미지 정보: {name}"))?,
        )
        .map_err(|e| e.to_string())
    };
    let (width, height, stride, format) = (
        number("width")?,
        number("height")?,
        number("stride")?,
        number("format")?,
    );
    let count = (width as usize)
        .checked_mul(height as usize)
        .filter(|n| *n > 0 && *n <= MAX_PIXELS)
        .ok_or("화면 이미지 크기가 유효하지 않습니다")?;
    let size = (stride as usize)
        .checked_mul(height as usize)
        .filter(|n| *n <= MAX_PIXELS * 4)
        .ok_or("화면 버퍼가 너무 큽니다")?;
    if (stride as u64) < (width as u64) * 4 || ![4, 5, 6, 16, 17, 18].contains(&format) {
        return Err(format!("지원하지 않는 화면 픽셀 형식: {format}"));
    }
    let mut raw = vec![0; size];
    reader
        .read_exact(&mut raw)
        .map_err(|e| format!("화면 이미지 수신 실패: {e}"))?;
    let mut pixels = Vec::with_capacity(count);
    for row in raw.chunks_exact(stride as usize) {
        for p in row[..width as usize * 4].as_chunks::<4>().0 {
            let pixel = if format <= 6 {
                u32::from_ne_bytes(*p)
            } else {
                ((p[3] as u32) << 24) | ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32
            };
            pixels.push(normalize(pixel, format));
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}
fn normalize(pixel: u32, format: u32) -> u32 {
    if format == 4 || format == 16 {
        return pixel | 0xff000000;
    }
    let alpha = pixel >> 24;
    if (format == 6 || format == 18) && alpha > 0 && alpha < 255 {
        let channel = |shift: u32| -> u32 { (((pixel >> shift) & 255) * 255 + alpha / 2) / alpha };
        (alpha << 24)
            | (channel(16).min(255) << 16)
            | (channel(8).min(255) << 8)
            | channel(0).min(255)
    } else {
        pixel
    }
}

pub fn clipboard(png: &[u8]) -> Result<(), String> {
    // wl-copy keeps ownership after its parent exits and releases it when a
    // newer clipboard source appears. No screenshot file is created.
    let mut child = Command::new("systemd-run")
        .args(["--user", "--scope", "--quiet", "--collect", "--unit"])
        .arg(format!("waytoys-clipboard-{}", std::process::id()))
        .args(
            if waytoys_desktop::Session::current()? == waytoys_desktop::Session::X11 {
                vec![
                    "xclip",
                    "-selection",
                    "clipboard",
                    "-in",
                    "-target",
                    "image/png",
                ]
            } else {
                vec!["wl-copy", "--type", "image/png"]
            },
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("클립보드 도구를 실행하지 못했습니다: {e}"))?;
    let mut input = child.stdin.take().unwrap();
    let bytes = png.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(input.write_all(&bytes));
    });
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {}
        failure => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("클립보드 이미지 전송 실패: {failure:?}"));
        }
    }
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            return Err(format!(
                "클립보드 복사 실패: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("클립보드 응답 시간이 초과되었습니다".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub fn notify_error(message: &str) {
    if let Ok(c) = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build())
        && let Ok(p) = Proxy::new(
            &c,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
    {
        let _: Result<u32, _> = p.call(
            "Notify",
            &(
                "Waytoys Capture",
                0u32,
                "camera-photo",
                "캡처하지 못했습니다",
                message,
                Vec::<String>::new(),
                HashMap::<String, Value>::new(),
                5000i32,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qimage_alpha_formats_are_converted_before_png_encoding() {
        assert_eq!(normalize(0x00123456, 4), 0xff123456);
        assert_eq!(normalize(0x80402010, 6), 0x80804020);
        assert_eq!(normalize(0xff123456, 6), 0xff123456);
        assert_eq!(normalize(0x80402010, 5), 0x80402010);
    }
}
