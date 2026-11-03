use crate::{
    Result,
    monitor::chosen_type,
    service::Shared,
    store::{MAX_IMAGE, Payload},
};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use waytoys_desktop::{
    clipboard::read_x11,
    x11::{
        Display,
        x11rb::{
            connection::Connection,
            protocol::{
                Event,
                xfixes::{ConnectionExt as _, SelectionEventMask},
                xproto::ConnectionExt as _,
            },
        },
    },
};

pub fn run(shared: Arc<Shared>) -> Result<()> {
    let display = Display::open().map_err(|e| e.to_string())?;
    let conn = &display.conn;
    conn.xfixes_query_version(5, 0)?.reply()?;
    let clipboard = display.atom("CLIPBOARD").map_err(|e| e.to_string())?;
    conn.xfixes_select_selection_input(
        display.root(),
        clipboard,
        SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY
            | SelectionEventMask::SELECTION_CLIENT_CLOSE,
    )?
    .check()?;
    conn.flush()?;
    shared.state.lock().unwrap().monitor_ready = true;
    let mut active = false;
    let mut changed = true;
    while !shared.stop.load(Ordering::SeqCst) {
        while let Some(event) = conn.poll_for_event()? {
            if matches!(event, Event::XfixesSelectionNotify(_)) {
                changed = true;
            }
        }
        let enabled =
            shared.enabled.load(Ordering::SeqCst) && !shared.locked.load(Ordering::SeqCst);
        if enabled && (changed || !active) {
            let restored = !changed;
            changed = false;
            let epoch = shared.epoch.load(Ordering::SeqCst);
            let owner = conn.get_selection_owner(clipboard)?.reply()?.owner;
            let result = (|| -> Result<()> {
                if owner == 0 {
                    return Ok(());
                }
                let targets = read_x11("TARGETS", 64 * 1024)?;
                let targets = String::from_utf8(targets)?
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let Some(target) = chosen_type(&targets) else {
                    return Ok(());
                };
                let bytes = read_x11(target, MAX_IMAGE)?;
                if bytes.is_empty() {
                    return Ok(());
                }
                let mime = if matches!(
                    target,
                    "UTF8_STRING" | "text/plain;charset=UTF-8" | "text/plain"
                ) {
                    "text/plain;charset=utf-8"
                } else {
                    target
                };
                let payload = Payload::parse(mime, bytes)?;
                // Owners may change while an INCR transfer is in progress, including
                // reassertions by the same window. Never mix two clipboard offers.
                let current = conn.get_selection_owner(clipboard)?.reply()?.owner;
                while let Some(event) = conn.poll_for_event()? {
                    if matches!(event, Event::XfixesSelectionNotify(_)) {
                        changed = true;
                    }
                }
                if current != owner
                    || changed
                    || !shared.enabled.load(Ordering::SeqCst)
                    || shared.locked.load(Ordering::SeqCst)
                    || epoch != shared.epoch.load(Ordering::SeqCst)
                {
                    return Ok(());
                }
                let mut state = shared.state.lock().unwrap();
                if epoch != shared.epoch.load(Ordering::SeqCst) {
                    return Ok(());
                }
                if let Some(store) = state.store.as_mut() {
                    store.ingest_current(&payload, restored)?;
                    state.error = None;
                }
                Ok(())
            })();
            if let Err(error) = result {
                shared.state.lock().unwrap().error = Some(format!("기록하지 못했습니다: {error}"));
            }
        }
        active = enabled;
        display
            .wait(Duration::from_millis(100))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
