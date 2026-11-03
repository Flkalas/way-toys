use std::{borrow::Cow, error::Error, sync::Arc};
pub use x11rb;
use x11rb::{
    connection::Connection,
    image::{BitsPerPixel, Image, ScanlinePad},
    protocol::{
        randr::ConnectionExt as _,
        shape::{ConnectionExt as _, SK, SO},
        xproto::*,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub name: String,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

pub struct Display {
    pub conn: RustConnection,
    pub screen: usize,
}
impl Display {
    pub fn open() -> Result<Arc<Self>> {
        let (conn, screen) = x11rb::connect(None)?;
        let version = conn.randr_query_version(1, 5)?.reply()?;
        if (version.major_version, version.minor_version) < (1, 5) {
            return Err("X11 백엔드에는 XRandR 1.5 이상이 필요합니다".into());
        }
        Ok(Arc::new(Self { conn, screen }))
    }
    pub fn root(&self) -> Window {
        self.conn.setup().roots[self.screen].root
    }
    pub fn atom(&self, name: &str) -> Result<Atom> {
        Ok(self.conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    }
    pub fn check_compositor(&self) -> Result<()> {
        let manager = self.atom(&format!("_NET_WM_CM_S{}", self.screen))?;
        if self.conn.get_selection_owner(manager)?.reply()?.owner == 0 {
            return Err("X11 오버레이를 사용하려면 KWin의 합성(Compositing)을 켜 주세요".into());
        }
        Ok(())
    }
    pub fn outputs(&self) -> Result<Vec<Output>> {
        let monitors = self.conn.randr_get_monitors(self.root(), true)?.reply()?;
        let mut outputs = Vec::new();
        for monitor in monitors.monitors {
            if monitor.width == 0 || monitor.height == 0 {
                continue;
            }
            outputs.push(Output {
                name: String::from_utf8(self.conn.get_atom_name(monitor.name)?.reply()?.name)?,
                x: monitor.x,
                y: monitor.y,
                width: monitor.width,
                height: monitor.height,
            });
        }
        outputs.sort_by(|a, b| a.name.cmp(&b.name));
        if outputs.is_empty() {
            return Err("연결된 XRandR 화면이 없습니다".into());
        }
        Ok(outputs)
    }
    pub fn wait(&self, duration: std::time::Duration) -> Result<()> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        let mut fds = [PollFd::new(self.conn.stream(), PollFlags::IN)];
        match poll(&mut fds, Some(&Timespec::try_from(duration)?)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn keysym(&self, keycode: u8) -> Result<u32> {
        Ok(self
            .conn
            .get_keyboard_mapping(keycode, 1)?
            .reply()?
            .keysyms
            .first()
            .copied()
            .unwrap_or(0))
    }
    pub fn screenshot(&self, output: &Output) -> Result<Vec<u32>> {
        if usize::from(output.width) * usize::from(output.height) > 64 * 1024 * 1024 {
            return Err("X11 화면 이미지가 너무 큽니다".into());
        }
        let screen = &self.conn.setup().roots[self.screen];
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|d| &d.visuals)
            .find(|v| v.visual_id == screen.root_visual)
            .ok_or("missing root visual")?;
        let (image, _) = Image::get(
            &self.conn,
            self.root(),
            output.x,
            output.y,
            output.width,
            output.height,
        )?;
        let channel = |pixel: u32, mask: u32| -> u32 {
            if mask == 0 {
                return 0;
            }
            let value = (pixel & mask) >> mask.trailing_zeros();
            (u64::from(value) * 255 / u64::from(mask >> mask.trailing_zeros())) as u32
        };
        let mut pixels = Vec::with_capacity(usize::from(output.width) * usize::from(output.height));
        for y in 0..output.height {
            for x in 0..output.width {
                let p = image.get_pixel(x, y);
                pixels.push(
                    0xff000000
                        | channel(p, visual.red_mask) << 16
                        | channel(p, visual.green_mask) << 8
                        | channel(p, visual.blue_mask),
                );
            }
        }
        Ok(pixels)
    }
}

/// Override-redirect overlay. Display-only surfaces have an empty input shape.
pub struct Surface {
    pub display: Arc<Display>,
    pub window: Window,
    pub output: Output,
    gc: Gcontext,
    colormap: Colormap,
    depth: u8,
    previous_focus: Window,
    restore_focus: bool,
}
impl Surface {
    pub fn new(
        display: Arc<Display>,
        output: Output,
        name: &str,
        input: bool,
        alpha: bool,
    ) -> Result<Self> {
        let conn = &display.conn;
        let screen = &conn.setup().roots[display.screen];
        let depth = if alpha { 32 } else { screen.root_depth };
        if alpha {
            display.check_compositor()?;
        }
        if usize::from(output.width) * usize::from(output.height) > 64 * 1024 * 1024 {
            return Err("X11 오버레이 이미지가 너무 큽니다".into());
        }
        let visual = screen
            .allowed_depths
            .iter()
            .filter(|d| d.depth == depth)
            .flat_map(|d| &d.visuals)
            .find(|v| {
                v.class == VisualClass::TRUE_COLOR
                    && v.red_mask == 0xff0000
                    && v.green_mask == 0xff00
                    && v.blue_mask == 0xff
            })
            .ok_or("X11 TrueColor RGB/ARGB visual이 필요합니다")?;
        let window = conn.generate_id()?;
        let gc = conn.generate_id()?;
        let colormap = conn.generate_id()?;
        conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual.visual_id)?
            .check()?;
        let events = EventMask::EXPOSURE
            | EventMask::STRUCTURE_NOTIFY
            | if input {
                EventMask::KEY_PRESS
                    | EventMask::KEY_RELEASE
                    | EventMask::BUTTON_PRESS
                    | EventMask::BUTTON_RELEASE
                    | EventMask::POINTER_MOTION
                    | EventMask::FOCUS_CHANGE
                    | EventMask::LEAVE_WINDOW
            } else {
                EventMask::NO_EVENT
            };
        conn.create_window(
            depth,
            window,
            screen.root,
            output.x,
            output.y,
            output.width,
            output.height,
            0,
            WindowClass::INPUT_OUTPUT,
            visual.visual_id,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(0)
                .border_pixel(0)
                .colormap(colormap)
                .event_mask(events),
        )?
        .check()?;
        conn.create_gc(gc, window, &CreateGCAux::new())?.check()?;
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            name.as_bytes(),
        )?;
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            b"waytoys\0waytoys\0",
        )?;
        if !input {
            conn.shape_rectangles(
                SO::SET,
                SK::INPUT,
                ClipOrdering::UNSORTED,
                window,
                0,
                0,
                &[],
            )?
            .check()?;
        }
        let previous_focus = conn.get_input_focus()?.reply()?.focus;
        Ok(Self {
            display,
            window,
            output,
            gc,
            colormap,
            depth,
            previous_focus,
            restore_focus: false,
        })
    }
    pub fn show(&mut self, focus: bool) -> Result<()> {
        let conn = &self.display.conn;
        conn.map_window(self.window)?.check()?;
        conn.configure_window(
            self.window,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?;
        if focus {
            conn.set_input_focus(InputFocus::POINTER_ROOT, self.window, x11rb::CURRENT_TIME)?
                .check()?;
            self.restore_focus = true;
        }
        conn.flush()?;
        Ok(())
    }
    pub fn grab(&self) -> Result<()> {
        let conn = &self.display.conn;
        if conn
            .grab_pointer(
                false,
                self.window,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                x11rb::NONE,
                x11rb::NONE,
                x11rb::CURRENT_TIME,
            )?
            .reply()?
            .status
            != GrabStatus::SUCCESS
        {
            return Err("다른 프로그램이 포인터를 사용 중입니다".into());
        }
        if conn
            .grab_keyboard(
                false,
                self.window,
                x11rb::CURRENT_TIME,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .reply()?
            .status
            != GrabStatus::SUCCESS
        {
            conn.ungrab_pointer(x11rb::CURRENT_TIME)?;
            return Err("다른 프로그램이 키보드를 사용 중입니다".into());
        }
        Ok(())
    }
    pub fn paint(&self, pixels: &[u8]) -> Result<()> {
        let order = if cfg!(target_endian = "little") {
            x11rb::image::ImageOrder::LsbFirst
        } else {
            x11rb::image::ImageOrder::MsbFirst
        };
        let image = Image::new(
            self.output.width,
            self.output.height,
            ScanlinePad::Pad32,
            self.depth,
            BitsPerPixel::B32,
            order,
            Cow::Borrowed(pixels),
        )?;
        for cookie in image.native(self.display.conn.setup())?.put(
            &self.display.conn,
            self.window,
            self.gc,
            0,
            0,
        )? {
            cookie.check()?;
        }
        self.display.conn.flush()?;
        Ok(())
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        let conn = &self.display.conn;
        if self.restore_focus
            && conn
                .get_input_focus()
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|r| r.focus == self.window)
        {
            let _ = conn.set_input_focus(
                InputFocus::POINTER_ROOT,
                self.previous_focus,
                x11rb::CURRENT_TIME,
            );
        }
        let _ = conn.destroy_window(self.window);
        let _ = conn.free_gc(self.gc);
        let _ = conn.free_colormap(self.colormap);
        let _ = conn.flush();
    }
}
