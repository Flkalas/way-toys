use crate::zones;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    sync::Arc,
};
use waytoys_desktop::x11::{
    Display,
    x11rb::{
        connection::Connection as _,
        protocol::{
            Event,
            xinput::{ConnectionExt as _, EventMask, XIEventMask},
            xproto::ConnectionExt as _,
        },
    },
};
use zbus::blocking::{Connection, Proxy};

pub struct Bridge {
    display: Arc<Display>,
    connection: Connection,
    pub epoch: String,
    pub owner: String,
    keys: BTreeMap<u32, u32>,
    input: InputState,
    _file: tempfile::NamedTempFile,
}
#[derive(serde::Deserialize)]
pub struct Input {
    #[serde(flatten)]
    pub event: zones::Event,
    pub app: String,
    #[serde(rename = "desktopFile")]
    pub desktop_file: String,
}
impl Bridge {
    pub fn start(connection: &Connection) -> Result<Self, String> {
        let create = (|| -> Result<Self, Box<dyn std::error::Error>> {
            let display = Display::open()?;
            let version = display.conn.xinput_xi_query_version(2, 1)?.reply()?;
            if (version.major_version, version.minor_version) < (2, 1) {
                return Err("X11 Zones에는 XInput 2.1 이상이 필요합니다".into());
            }
            display
                .conn
                .xinput_xi_select_events(
                    display.root(),
                    &[EventMask {
                        deviceid: 1,
                        mask: vec![
                            XIEventMask::RAW_KEY_PRESS
                                | XIEventMask::RAW_KEY_RELEASE
                                | XIEventMask::RAW_BUTTON_PRESS
                                | XIEventMask::RAW_BUTTON_RELEASE,
                        ],
                    }],
                )?
                .check()?;
            let setup = display.conn.setup();
            let mapping = display
                .conn
                .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)?
                .reply()?;
            let keys: BTreeMap<_, _> = mapping
                .keysyms
                .chunks(usize::from(mapping.keysyms_per_keycode))
                .enumerate()
                .map(|(index, syms)| (u32::from(setup.min_keycode) + index as u32, syms[0]))
                .collect();
            let pressed = display.conn.query_keymap()?.reply()?.keys;
            let mut input = InputState::default();
            for (&code, &symbol) in &keys {
                if pressed[code as usize / 8] & (1 << (code % 8)) != 0 {
                    input.key(symbol, true);
                }
            }
            let bus = Proxy::new(
                connection,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
            )?;
            let owner: String = bus.call("GetNameOwner", &("org.kde.KWin",))?;
            let epoch = format!(
                "x11-{}",
                std::fs::read_to_string("/proc/sys/kernel/random/uuid")?.trim()
            );
            let mut file = tempfile::NamedTempFile::new()?;
            writeln!(file, "const epoch = {};", serde_json::to_string(&epoch)?)?;
            file.write_all(include_bytes!("../../../kwin/x11/zones.js"))?;
            file.flush()?;
            let scripting = Proxy::new(
                connection,
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting",
            )?;
            let _: bool = scripting.call("unloadScript", &("waytoys-x11-zones",))?;
            let id: i32 = scripting.call(
                "loadScript",
                &(
                    file.path().to_str().ok_or("invalid script path")?,
                    "waytoys-x11-zones",
                ),
            )?;
            if id < 0 {
                return Err("KWin X11 스크립트를 불러오지 못했습니다".into());
            }
            let script = Proxy::new(
                connection,
                "org.kde.KWin",
                format!("/Scripting/Script{id}"),
                "org.kde.kwin.Script",
            )?;
            let _: () = script.call("run", &())?;
            Ok(Self {
                display,
                connection: connection.clone(),
                epoch,
                owner,
                keys,
                input,
                _file: file,
            })
        })();
        create.map_err(|e| e.to_string())
    }
    pub fn outputs(&self) -> Result<Vec<zones::Output>, String> {
        self.display.check_compositor().map_err(|e| e.to_string())?;
        self.display
            .outputs()
            .map(|outputs| {
                outputs
                    .into_iter()
                    .map(|o| zones::Output {
                        name: o.name,
                        geometry: [
                            i32::from(o.x),
                            i32::from(o.y),
                            i32::from(o.width),
                            i32::from(o.height),
                        ],
                    })
                    .collect()
            })
            .map_err(|e| e.to_string())
    }
    pub fn healthy(&self) -> Result<bool, String> {
        let check = (|| -> Result<bool, zbus::Error> {
            let bus = Proxy::new(
                &self.connection,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
            )?;
            let owner: String = bus.call("GetNameOwner", &("org.kde.KWin",))?;
            if owner != self.owner {
                return Ok(false);
            }
            let scripting = Proxy::new(
                &self.connection,
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting",
            )?;
            scripting.call("isScriptLoaded", &("waytoys-x11-zones",))
        })();
        check.map_err(|e| e.to_string())
    }
    pub fn input(&mut self, drop: bool) -> Result<(bool, bool, bool), String> {
        // A round trip places all preceding input in this connection's event
        // queue, even when KWin's native move owns the keyboard/pointer grab.
        self.display
            .conn
            .get_input_focus()
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        while let Some(event) = self
            .display
            .conn
            .poll_for_event()
            .map_err(|e| e.to_string())?
        {
            match event {
                Event::XinputRawKeyPress(e) => self
                    .input
                    .key(*self.keys.get(&e.detail).unwrap_or(&0), true),
                Event::XinputRawKeyRelease(e) => self
                    .input
                    .key(*self.keys.get(&e.detail).unwrap_or(&0), false),
                Event::XinputRawButtonPress(e) if e.detail == 1 => self.input.button(true),
                Event::XinputRawButtonRelease(e) if e.detail == 1 => self.input.button(false),
                _ => {}
            }
        }
        Ok(self.input.sample(drop))
    }
}

/// Only Shift, Ctrl, Escape and the left-button transition are retained. Text
/// keys are ignored. Remember release modifiers so a fast Shift release cannot
/// turn a completed zone drop into a normal move.
#[derive(Default)]
struct InputState {
    pressed: BTreeSet<u32>,
    left: bool,
    cancelled: bool,
    released: Option<(bool, bool, bool)>,
}
impl InputState {
    fn key(&mut self, symbol: u32, down: bool) {
        if matches!(symbol, 0xffe1..=0xffe4) {
            if down {
                self.pressed.insert(symbol);
            } else {
                self.pressed.remove(&symbol);
            }
        }
        if symbol == 0xff1b && down && self.left {
            self.cancelled = true;
        }
    }
    fn button(&mut self, down: bool) {
        self.left = down;
        if down {
            self.cancelled = false;
            self.released = None;
        } else {
            self.released = Some(self.sample(false));
        }
    }
    fn sample(&self, drop: bool) -> (bool, bool, bool) {
        if drop && let Some(released) = self.released {
            return released;
        }
        (
            self.pressed.contains(&0xffe1) || self.pressed.contains(&0xffe2),
            self.pressed.contains(&0xffe3) || self.pressed.contains(&0xffe4),
            self.cancelled,
        )
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if let Ok(p) = Proxy::new(
            &self.connection,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        ) {
            let _: Result<bool, _> = p.call("unloadScript", &("waytoys-x11-zones",));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InputState;
    #[test]
    fn escape_is_retained_after_release_and_reset_for_the_next_drag() {
        let mut state = InputState::default();
        state.button(true);
        state.key(0xffe1, true);
        state.key(0xff1b, true);
        state.key(0xff1b, false);
        state.button(false);
        state.key(0xffe1, false);
        assert_eq!(state.sample(true), (true, false, true));
        state.button(true);
        assert_eq!(state.sample(false), (false, false, false));
    }
    #[test]
    fn release_modifiers_distinguish_drop_from_shift_released_before_drop() {
        let mut state = InputState::default();
        state.button(true);
        state.key(0xffe1, true);
        state.button(false);
        state.key(0xffe1, false);
        assert_eq!(state.sample(true), (true, false, false));
        state.button(true);
        state.key(0xffe1, true);
        state.key(0xffe1, false);
        state.button(false);
        assert_eq!(state.sample(true), (false, false, false));
    }
}
