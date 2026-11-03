mod backend;
mod image;
mod overlay;
mod x11;
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use zbus::{
    blocking::{Proxy, connection::Builder},
    fdo::{RequestNameFlags, RequestNameReply},
};

struct Control {
    stop: Arc<AtomicBool>,
}
#[zbus::interface(name = "org.waytoys.Capture1")]
impl Control {
    fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let check = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [arg] if arg == "--check" => true,
        [arg] if arg == "--help" || arg == "-h" => {
            println!(
                "waytoys-capture [--check]\n드래그한 영역을 PNG 이미지로 클립보드에 복사합니다. Esc 또는 오른쪽 클릭: 취소.\n--check: 화면 획득만 확인하며 클립보드를 바꾸지 않습니다."
            );
            return Ok(());
        }
        _ => return Err("usage: waytoys-capture [--check]".into()),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let bus = Builder::session()?
        .method_timeout(Duration::from_secs(6))
        .serve_at("/Capture", Control { stop: stop.clone() })?
        .build()?;
    let owner = match bus
        .request_name_with_flags("org.waytoys.Capture1", RequestNameFlags::DoNotQueue.into())
    {
        Ok(owner) => owner,
        Err(zbus::Error::NameTaken) => {
            println!("capture already running");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if owner != RequestNameReply::PrimaryOwner {
        println!("capture already running");
        return Ok(());
    }
    let saver = Proxy::new(
        &bus,
        "org.freedesktop.ScreenSaver",
        "/ScreenSaver",
        "org.freedesktop.ScreenSaver",
    )?;
    let locked: bool = saver.call("GetActive", &())?;
    if locked {
        return Err("화면 잠금을 해제한 뒤 캡처를 시작하세요".into());
    }
    let watch_bus = bus.clone();
    let watch_stop = stop.clone();
    std::thread::spawn(move || {
        if let Ok(proxy) = Proxy::new(
            &watch_bus,
            "org.freedesktop.ScreenSaver",
            "/ScreenSaver",
            "org.freedesktop.ScreenSaver",
        ) && let Ok(events) = proxy.receive_signal("ActiveChanged")
        {
            for event in events {
                if event.body().deserialize::<bool>().unwrap_or(true) {
                    watch_stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    });
    let dbus = Proxy::new(
        &bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let runtime: bool = dbus.call("NameHasOwner", &("org.waytoys.Runtime1",))?;
    if runtime {
        let p = Proxy::new(
            &bus,
            "org.waytoys.Runtime1",
            "/Waytoys",
            "org.waytoys.Runtime1",
        )?;
        let _: String = p.call("SuspendForCapture", &("",))?;
    }
    // Let the invoking tray menu or settings-window minimize animation finish.
    std::thread::sleep(Duration::from_millis(200));
    let image = match waytoys_desktop::Session::current()? {
        waytoys_desktop::Session::X11 => x11::run(check, stop.clone())?,
        waytoys_desktop::Session::Wayland => overlay::run(&bus, check, stop.clone())?,
    };
    if let Some(image) = image {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let png = image.png()?;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        backend::clipboard(&png)?;
        println!(
            "copied image/png {}x{} bytes={}",
            image.width,
            image.height,
            png.len()
        );
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("waytoys-capture: {e}");
        backend::notify_error(&e.to_string());
        std::process::exit(1);
    }
}
