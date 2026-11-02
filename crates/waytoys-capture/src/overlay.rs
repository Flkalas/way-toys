//! Frozen, per-output layer surfaces. Input coordinates remain desktop-logical;
//! buffers use the original screenshots' physical pixels via viewporter.
use crate::{
    backend,
    image::{self, Image, Rect, Screen},
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::wp::{
    cursor_shape::v1::client::{
        wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
        wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

struct Surface {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    viewport: WpViewport,
    pool: SlotPool,
    screen: usize,
    configured: bool,
    pending: bool,
    dirty: bool,
}
impl Drop for Surface {
    fn drop(&mut self) {
        self.viewport.destroy();
    }
}
struct App {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    shm: Shm,
    surfaces: Vec<Surface>,
    screens: Vec<Screen>,
    cursor_manager: Option<WpCursorShapeManagerV1>,
    cursor: Option<WpCursorShapeDeviceV1>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    bounds: Rect,
    drag: Option<(f64, f64)>,
    selected: Option<Rect>,
    accepted: bool,
    exit: bool,
    active: bool,
    keyboard_entered: bool,
    error: Option<String>,
}

pub fn run(
    bus: &zbus::blocking::Connection,
    check: bool,
    stop: Arc<AtomicBool>,
) -> Result<Option<Image>, Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)?;
    let layers = LayerShell::bind(&globals, &qh)?;
    let viewporter: WpViewporter = globals.bind(&qh, 1..=1, ())?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        shm: Shm::bind(&globals, &qh)?,
        surfaces: vec![],
        screens: vec![],
        cursor_manager: globals.bind(&qh, 1..=1, ()).ok(),
        cursor: None,
        keyboard: None,
        pointer: None,
        bounds: Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        drag: None,
        selected: None,
        accepted: false,
        exit: false,
        active: false,
        keyboard_entered: false,
        error: None,
    };
    queue.roundtrip(&mut app)?;
    queue.roundtrip(&mut app)?;
    let mut outputs: Vec<_> = app
        .outputs
        .outputs()
        .filter_map(|o| app.outputs.info(&o).map(|i| (o, i)))
        .collect();
    outputs.sort_by_key(|(_, i)| i.logical_position.unwrap_or(i.location));
    if outputs.is_empty() {
        return Err("연결된 화면이 없습니다".into());
    }
    let mut pixels = 0usize;
    for (_, info) in &outputs {
        let name = info.name.as_ref().ok_or("화면 이름을 확인할 수 없습니다")?;
        let (x, y) = info
            .logical_position
            .ok_or("화면의 논리 위치를 확인할 수 없습니다")?;
        let (w, h) = info
            .logical_size
            .ok_or("화면의 논리 크기를 확인할 수 없습니다")?;
        if w <= 0 || h <= 0 {
            return Err("화면 크기가 유효하지 않습니다".into());
        }
        let image = backend::screenshot(bus, name)?;
        pixels = pixels
            .checked_add(image.pixels.len())
            .filter(|n| *n <= image::MAX_PIXELS)
            .ok_or("전체 화면 이미지가 너무 큽니다")?;
        println!(
            "captured output={name} logical={x},{y},{w},{h} pixels={}x{}",
            image.width, image.height
        );
        app.screens.push(Screen::new(
            name.clone(),
            Rect {
                x,
                y,
                width: w as u32,
                height: h as u32,
            },
            image,
        ));
    }
    if check {
        return Ok(None);
    }
    let x = app.screens.iter().map(|s| s.rect.x).min().unwrap();
    let y = app.screens.iter().map(|s| s.rect.y).min().unwrap();
    let right = app
        .screens
        .iter()
        .map(|s| s.rect.right())
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = app
        .screens
        .iter()
        .map(|s| s.rect.bottom())
        .fold(f64::NEG_INFINITY, f64::max);
    app.bounds = Rect {
        x,
        y,
        width: (right - x as f64) as u32,
        height: (bottom - y as f64) as u32,
    };
    for (index, (output, _)) in outputs.into_iter().enumerate() {
        let surface = compositor.create_surface(&qh);
        let viewport = viewporter.get_viewport(&surface, &qh, ());
        let layer = layers.create_layer_surface(
            &qh,
            surface,
            Layer::Overlay,
            Some("waytoys-capture"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.set_keyboard_interactivity(if index == 0 {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        });
        let pool = SlotPool::new(4, &app.shm)?;
        app.surfaces.push(Surface {
            output,
            layer,
            viewport,
            pool,
            screen: index,
            configured: false,
            pending: false,
            dirty: true,
        });
        app.surfaces.last().unwrap().layer.commit();
    }
    app.active = true;
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, stop.clone())?;
    }
    while !app.exit && !stop.load(Ordering::Relaxed) {
        queue.dispatch_pending(&mut app)?;
        if app.exit {
            break;
        }
        app.draw(&qh)?;
        conn.flush()?;
        if let Some(guard) = queue.prepare_read() {
            let timeout = Timespec::try_from(Duration::from_millis(250))?;
            let mut fds = [PollFd::new(&conn, PollFlags::IN)];
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
    app.active = false;
    app.surfaces.clear();
    conn.flush()?;
    if let Some(error) = app.error {
        return Err(error.into());
    }
    if app.accepted && !stop.load(Ordering::Relaxed) {
        let region = app.selected.ok_or("선택 영역이 없습니다")?;
        println!(
            "selection x={} y={} width={} height={}",
            region.x, region.y, region.width, region.height
        );
        return Ok(Some(image::compose(&app.screens, region)?));
    }
    println!("capture cancelled");
    Ok(None)
}
impl App {
    fn dirty(&mut self) {
        for s in &mut self.surfaces {
            s.dirty = true;
        }
    }
    fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        for s in &mut self.surfaces {
            if !s.configured || s.pending || !s.dirty {
                continue;
            }
            let screen = &self.screens[s.screen];
            let width = i32::try_from(screen.image.width)?;
            let height = i32::try_from(screen.image.height)?;
            let (buffer, canvas) = s.pool.create_buffer(
                width,
                height,
                width.checked_mul(4).ok_or("buffer overflow")?,
                wl_shm::Format::Argb8888,
            )?;
            screen.paint(canvas, self.selected);
            s.layer.wl_surface().set_buffer_scale(1);
            s.viewport
                .set_destination(screen.rect.width as i32, screen.rect.height as i32);
            buffer.attach_to(s.layer.wl_surface())?;
            s.layer.wl_surface().damage_buffer(0, 0, width, height);
            s.layer.wl_surface().frame(qh, s.layer.wl_surface().clone());
            s.layer.commit();
            s.pending = true;
            s.dirty = false;
        }
        Ok(())
    }
    fn point(&self, event: &PointerEvent) -> Option<(f64, f64)> {
        let s = self
            .surfaces
            .iter()
            .find(|s| s.layer.wl_surface() == &event.surface)?;
        let r = self.screens[s.screen].rect;
        Some((r.x as f64 + event.position.0, r.y as f64 + event.position.1))
    }
}
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if self.active {
            self.exit = true;
        }
    }
    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.active
            && let Some(s) = self.surfaces.iter().find(|s| s.output == output)
            && let Some(i) = self.outputs.info(&output)
        {
            let r = self.screens[s.screen].rect;
            if i.logical_position != Some((r.x, r.y))
                || i.logical_size != Some((r.width as i32, r.height as i32))
            {
                self.exit = true;
            }
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        if self.active {
            self.exit = true;
        }
    }
}
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        c: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(s) = self.surfaces.iter_mut().find(|s| s.layer == *layer) {
            let r = self.screens[s.screen].rect;
            if c.new_size != (r.width, r.height) {
                self.error = Some("화면 크기가 변경되어 캡처를 취소했습니다".into());
                self.exit = true;
                return;
            }
            s.configured = true;
            s.dirty = true;
            println!("selection ready output={}", self.screens[s.screen].name);
        }
    }
}
impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(s) = self
            .surfaces
            .iter_mut()
            .find(|s| s.layer.wl_surface() == surface)
        {
            s.pending = false;
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard if self.keyboard.is_none() => {
                self.keyboard = self.seats.get_keyboard(qh, &seat, None).ok()
            }
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seats.get_pointer(qh, &seat).ok();
                if let (Some(manager), Some(pointer)) = (&self.cursor_manager, &self.pointer) {
                    self.cursor = Some(manager.get_pointer(pointer, qh, ()));
                }
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
        self.exit = true;
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {
        self.exit = true;
    }
}
impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.keyboard_entered = true;
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.keyboard_entered {
            self.exit = true;
        }
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        e: KeyEvent,
    ) {
        if e.keysym == Keysym::Escape {
            self.exit = true;
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}
impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for e in events {
            if self.exit {
                break;
            }
            let Some(point) = self.point(e) else {
                continue;
            };
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some(cursor) = &self.cursor {
                        cursor.set_shape(serial, Shape::Crosshair);
                    }
                }
                PointerEventKind::Press { button: 0x111, .. } => self.exit = true,
                PointerEventKind::Press { button: 0x110, .. } => {
                    self.drag = Some(point);
                    self.selected = None;
                    self.dirty();
                }
                PointerEventKind::Motion { .. } => {
                    if let Some(start) = self.drag {
                        self.selected = image::selection(start, point, self.bounds);
                        self.dirty();
                    }
                }
                PointerEventKind::Release { button: 0x110, .. } => {
                    if let Some(start) = self.drag.take() {
                        self.selected = image::selection(start, point, self.bounds)
                            .filter(|r| self.screens.iter().any(|s| s.rect.intersects(*r)));
                        if self.selected.is_some() {
                            self.accepted = true;
                            self.exit = true;
                        } else {
                            self.dirty();
                        }
                    }
                }
                PointerEventKind::Leave { .. } if self.drag.is_some() => self.exit = true,
                _ => {}
            }
        }
    }
}
impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_keyboard!(App);
delegate_pointer!(App);
delegate_layer!(App);
delegate_registry!(App);
wayland_client::delegate_noop!(App: ignore WpViewporter);
wayland_client::delegate_noop!(App: ignore WpViewport);
wayland_client::delegate_noop!(App: ignore WpCursorShapeManagerV1);
wayland_client::delegate_noop!(App: ignore WpCursorShapeDeviceV1);
