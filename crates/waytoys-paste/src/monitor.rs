//! Data-control observes clipboard offers, never global keyboard input.
use crate::{
    Result,
    service::Shared,
    store::{MAX_IMAGE, Payload},
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::{
    io::Read,
    os::{fd::AsFd, unix::net::UnixStream},
    sync::{Arc, Mutex, atomic::Ordering},
    time::Duration,
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
};

pub fn chosen_type(types: &[String]) -> Option<&str> {
    if types
        .iter()
        .any(|t| t == "x-kde-passwordManagerHint" || t == "application/x-keepassxc")
    {
        return None;
    }
    [
        "image/png",
        "image/jpeg",
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "UTF8_STRING",
        "text/plain",
    ]
    .into_iter()
    .find(|t| types.iter().any(|v| v == t))
}
struct App {
    shared: Arc<Shared>,
    current: Option<ZwlrDataControlOfferV1>,
    active: bool,
    finished: bool,
    initial: bool,
}
impl App {
    fn receive(&mut self, connection: &Connection, restored: bool) {
        if !self.shared.enabled.load(Ordering::SeqCst) || self.shared.locked.load(Ordering::SeqCst)
        {
            return;
        }
        let Some(offer) = self.current.as_ref() else {
            return;
        };
        let types = offer
            .data::<Mutex<Vec<String>>>()
            .unwrap()
            .lock()
            .unwrap()
            .clone();
        let Some(mime) = chosen_type(&types) else {
            return;
        };
        let epoch = self.shared.epoch.load(Ordering::SeqCst);
        let result = (|| -> Result<()> {
            let (reader, writer) = UnixStream::pair()?;
            reader.set_read_timeout(Some(Duration::from_secs(2)))?;
            offer.receive(mime.into(), writer.as_fd());
            connection.flush()?;
            drop(writer);
            let mut bytes = Vec::new();
            reader
                .take((MAX_IMAGE + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                return Ok(());
            }
            let payload = Payload::parse(mime, bytes)?;
            if !self.shared.enabled.load(Ordering::SeqCst)
                || self.shared.locked.load(Ordering::SeqCst)
                || epoch != self.shared.epoch.load(Ordering::SeqCst)
            {
                return Ok(());
            }
            let mut state = self.shared.state.lock().unwrap();
            if epoch != self.shared.epoch.load(Ordering::SeqCst) {
                return Ok(());
            }
            if let Some(store) = state.store.as_mut() {
                store.ingest_current(&payload, restored)?;
                state.error = None;
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.shared.state.lock().unwrap().error = Some(format!("기록하지 못했습니다: {e}"));
        }
    }
}
pub fn run(shared: Arc<Shared>) -> Result<()> {
    if waytoys_desktop::Session::current()? == waytoys_desktop::Session::X11 {
        return crate::x11::run(shared);
    }
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&connection)?;
    let qh = queue.handle();
    let manager: ZwlrDataControlManagerV1 = globals.bind(&qh, 1..=2, ())?;
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=9, ())?;
    let device = manager.get_data_device(&seat, &qh, ());
    let mut app = App {
        active: shared.enabled.load(Ordering::SeqCst),
        shared,
        current: None,
        finished: false,
        initial: true,
    };
    queue.roundtrip(&mut app)?;
    app.shared.state.lock().unwrap().monitor_ready = true;
    while !app.shared.stop.load(Ordering::SeqCst) && !app.finished {
        queue.dispatch_pending(&mut app)?;
        let active =
            app.shared.enabled.load(Ordering::SeqCst) && !app.shared.locked.load(Ordering::SeqCst);
        if active && !app.active {
            app.receive(&connection, true);
        }
        app.active = active;
        connection.flush()?;
        if let Some(guard) = queue.prepare_read() {
            let timeout = Timespec::try_from(Duration::from_millis(200))?;
            let mut fds = [PollFd::new(&connection, PollFlags::IN)];
            match poll(&mut fds, Some(&timeout)) {
                Ok(0) => drop(guard),
                Ok(_) => {
                    guard.read()?;
                }
                Err(rustix::io::Errno::INTR) => drop(guard),
                Err(e) => return Err(e.into()),
            }
        }
    }
    if let Some(offer) = app.current.take() {
        offer.destroy();
    }
    device.destroy();
    manager.destroy();
    connection.flush()?;
    if app.finished {
        return Err("클립보드 연결이 종료됐습니다".into());
    }
    Ok(())
}
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZwlrDataControlDeviceV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        connection: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::Selection { id } => {
                if let Some(old) = app.current.take() {
                    old.destroy();
                }
                app.current = id;
                app.receive(connection, app.initial);
                app.initial = false;
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id: Some(id) } => id.destroy(),
            zwlr_data_control_device_v1::Event::Finished => app.finished = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(App,ZwlrDataControlDeviceV1,[0=>(ZwlrDataControlOfferV1,Mutex::new(Vec::<String>::new()))]);
}
impl Dispatch<ZwlrDataControlOfferV1, Mutex<Vec<String>>> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        types: &Mutex<Vec<String>>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event
            && [
                "image/png",
                "image/jpeg",
                "text/plain;charset=utf-8",
                "text/plain;charset=UTF-8",
                "UTF8_STRING",
                "text/plain",
                "x-kde-passwordManagerHint",
                "application/x-keepassxc",
            ]
            .contains(&mime_type.as_str())
        {
            let mut types = types.lock().unwrap();
            if !types.contains(&mime_type) {
                types.push(mime_type);
            }
        }
    }
}
wayland_client::delegate_noop!(App:ignore ZwlrDataControlManagerV1);
wayland_client::delegate_noop!(App:ignore wl_seat::WlSeat);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mime_precedence_ignores_sensitive_offers_and_primary_selection() {
        let types = vec!["text/plain".into(), "image/png".into()];
        assert_eq!(chosen_type(&types), Some("image/png"));
        let mut secret = types;
        secret.push("x-kde-passwordManagerHint".into());
        assert_eq!(chosen_type(&secret), None);
        assert_eq!(chosen_type(&["text/uri-list".into()]), None);
    }
}
