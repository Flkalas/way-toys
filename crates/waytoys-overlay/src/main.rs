//! Minimal per-output Wayland overlay experiment.
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
use std::error::Error;
use wayland_client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Display,
    Select,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
fn rectangle(a: (f64, f64), b: (f64, f64)) -> Rect {
    Rect {
        x: a.0.min(b.0),
        y: a.1.min(b.1),
        width: (a.0 - b.0).abs(),
        height: (a.1 - b.1).abs(),
    }
}
/// Convert logical surface dimensions to integer-scaled buffer dimensions.
fn buffer_dimensions(size: (u32, u32), scale: i32) -> Option<(i32, i32, i32)> {
    let scale = u32::try_from(scale).ok().filter(|scale| *scale > 0)?;
    let width = size.0.checked_mul(scale)?;
    let height = size.1.checked_mul(scale)?;
    let stride = width.checked_mul(4)?;
    Some((
        i32::try_from(width).ok()?,
        i32::try_from(height).ok()?,
        i32::try_from(stride).ok()?,
    ))
}

fn paint_overlay(canvas: &mut [u8], size: (u32, u32), scale: u32, index: usize) {
    // A reused SHM slot may be larger than the current image.
    let (width, height) = (size.0 as usize, size.1 as usize);
    let colors = [0x70301040u32, 0x70104030u32, 0x70403010u32];
    let border_colors = [0xffd68cffu32, 0xff70e0b0u32, 0xffffbc70u32];
    let color = colors[index % colors.len()].to_ne_bytes();
    let border_color = border_colors[index % border_colors.len()].to_ne_bytes();
    let border = (2 * scale) as usize;
    for (idx, px) in canvas
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .take(width * height)
        .enumerate()
    {
        let x = idx % width;
        let y = idx / width;
        let on_border =
            x < border || y < border || width - 1 - x < border || height - 1 - y < border;
        px.copy_from_slice(if on_border { &border_color } else { &color });
    }
}

struct Overlay {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    pool: SlotPool,
    size: (u32, u32),
    scale: i32,
    index: usize,
    frame_pending: bool,
    dirty: bool,
}
struct App {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    layers: LayerShell,
    shm: Shm,
    surfaces: Vec<Overlay>,
    mode: Mode,
    exit: bool,
    error: Option<String>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    drag: Option<(wl_surface::WlSurface, (f64, f64))>,
    drag_position: Option<(f64, f64)>,
    next_index: usize,
}
fn main() {
    if let Err(error) = run() {
        eprintln!("waytoys-overlay: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn Error>> {
    let mode = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => Mode::Display,
        [arg] if arg == "--display" => Mode::Display,
        [arg] if arg == "--select" => Mode::Select,
        [arg] if arg == "--help" => {
            println!(
                "Usage: waytoys-overlay [--display | --select]\nDisplay mode passes input through; select mode prints output-local logical selection coordinates."
            );
            return Ok(());
        }
        _ => return Err("usage: waytoys-overlay [--display | --select]".into()),
    };
    let conn =
        Connection::connect_to_env().map_err(|e| format!("Wayland connection unavailable: {e}"))?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|e| format!("wl_compositor unavailable: {e}"))?;
    let layers =
        LayerShell::bind(&globals, &qh).map_err(|e| format!("wlr-layer-shell unavailable: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm unavailable: {e}"))?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        compositor,
        layers,
        shm,
        surfaces: vec![],
        mode,
        exit: false,
        error: None,
        keyboard: None,
        pointer: None,
        drag: None,
        drag_position: None,
        next_index: 0,
    };
    // Output callbacks are delivered by the event queue after globals have been bound.
    while !app.exit {
        queue.blocking_dispatch(&mut app)?;
    }
    if let Some(error) = app.error.take() {
        return Err(error.into());
    }
    println!("overlay closed");
    Ok(())
}
impl App {
    fn add_output(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if self.surfaces.iter().any(|s| s.output == output) {
            return;
        }
        let surface = self.compositor.create_surface(qh);
        if self.mode == Mode::Display {
            let region = self.compositor.wl_compositor().create_region(qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();
        }
        let layer = self.layers.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("waytoys-overlay"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        layer.set_keyboard_interactivity(
            if self.mode == Mode::Select && self.surfaces.is_empty() {
                KeyboardInteractivity::Exclusive
            } else {
                KeyboardInteractivity::None
            },
        );
        layer.commit();
        let index = self.next_index;
        self.next_index += 1;
        match SlotPool::new(4, &self.shm) {
            Ok(pool) => self.surfaces.push(Overlay {
                output,
                layer,
                pool,
                size: (0, 0),
                scale: 1,
                index,
                frame_pending: false,
                dirty: false,
            }),
            Err(e) => {
                self.error = Some(format!("cannot allocate shared memory: {e}"));
                self.exit = true;
            }
        }
    }
    fn draw(&mut self, idx: usize, qh: &QueueHandle<Self>) {
        let s = &mut self.surfaces[idx];
        if s.frame_pending {
            s.dirty = true;
            return;
        }
        s.dirty = false;
        let selection = self.drag.as_ref().and_then(|(surface, start)| {
            (surface == s.layer.wl_surface())
                .then(|| self.drag_position.map(|end| rectangle(*start, end)))
                .flatten()
        });
        let (w, h) = s.size;
        if w == 0 || h == 0 {
            return;
        }
        let scale = s.scale.max(1) as u32;
        let Some((iw, ih, stride)) = buffer_dimensions((w, h), s.scale) else {
            self.error = Some(format!(
                "invalid or oversized surface dimensions: {w}x{h} scale {}",
                s.scale
            ));
            self.exit = true;
            return;
        };
        let (pw, ph) = (iw as u32, ih as u32);
        let (buffer, canvas) = match s
            .pool
            .create_buffer(iw, ih, stride, wl_shm::Format::Argb8888)
        {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(format!("cannot create buffer: {e}"));
                self.exit = true;
                return;
            }
        };
        // ARGB premultiplied alpha; byte order follows native endianness.
        paint_overlay(canvas, (pw, ph), scale, s.index);
        if let Some(r) = selection {
            let x0 = (r.x * f64::from(scale)).max(0.0).min(f64::from(pw)) as u32;
            let y0 = (r.y * f64::from(scale)).max(0.0).min(f64::from(ph)) as u32;
            let x1 = ((r.x + r.width) * f64::from(scale))
                .max(0.0)
                .min(f64::from(pw)) as u32;
            let y1 = ((r.y + r.height) * f64::from(scale))
                .max(0.0)
                .min(f64::from(ph)) as u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let edge = x - x0 < 2 * scale
                        || y - y0 < 2 * scale
                        || x1 - x <= 2 * scale
                        || y1 - y <= 2 * scale;
                    let pixel: u32 = if edge { 0xffeeeeee } else { 0x50303030 };
                    let offset = ((y as usize * pw as usize) + x as usize) * 4;
                    canvas[offset..offset + 4].copy_from_slice(&pixel.to_ne_bytes());
                }
            }
        }
        s.layer.wl_surface().set_buffer_scale(s.scale.max(1));
        s.layer.wl_surface().damage_buffer(0, 0, iw, ih);
        if let Err(e) = buffer.attach_to(s.layer.wl_surface()) {
            self.error = Some(format!("cannot attach buffer: {e}"));
            self.exit = true;
            return;
        }
        s.layer.wl_surface().frame(qh, s.layer.wl_surface().clone());
        s.frame_pending = true;
        s.layer.commit();
        println!(
            "surface configured output={} logical={}x{} scale={} buffer={}x{}",
            s.index, w, h, s.scale, pw, ph
        );
    }
    fn redraw_surface(
        &mut self,
        surface: &wl_surface::WlSurface,
        scale: i32,
        qh: &QueueHandle<Self>,
    ) {
        if let Some(idx) = self
            .surfaces
            .iter()
            .position(|s| s.layer.wl_surface() == surface)
        {
            self.surfaces[idx].scale = scale.max(1);
            self.draw(idx, qh);
        }
    }
}
impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        s: &wl_surface::WlSurface,
        scale: i32,
    ) {
        self.redraw_surface(s, scale, qh)
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
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(idx) = self
            .surfaces
            .iter()
            .position(|s| s.layer.wl_surface() == surface)
        {
            self.surfaces[idx].frame_pending = false;
            if self.surfaces[idx].dirty {
                self.draw(idx, qh);
            }
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
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, o: wl_output::WlOutput) {
        if let Some(i) = self.outputs.info(&o) {
            println!(
                "output added id={} name={:?} logical_size={:?} scale={}",
                i.id, i.name, i.logical_size, i.scale_factor
            );
        }
        self.add_output(qh, o)
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        if let Some(i) = self.outputs.info(&o) {
            println!(
                "output updated id={} name={:?} logical_size={:?} scale={}",
                i.id, i.name, i.logical_size, i.scale_factor
            )
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        if let Some(i) = self.surfaces.iter().position(|s| s.output == o) {
            let removed = self.surfaces.remove(i);
            println!("output removed index={}", removed.index);
            if self
                .drag
                .as_ref()
                .is_some_and(|(s, _)| s == removed.layer.wl_surface())
            {
                self.drag = None;
                self.drag_position = None;
                println!("selection cancelled: output removed");
            }
        }
        if self.mode == Mode::Select
            && let Some(first) = self.surfaces.first()
        {
            first
                .layer
                .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            first.layer.commit();
        }
    }
}
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(i) = self.surfaces.iter().position(|s| s.layer == *layer) {
            let removed = self.surfaces.remove(i);
            if self
                .drag
                .as_ref()
                .is_some_and(|(surface, _)| surface == removed.layer.wl_surface())
            {
                self.drag = None;
                self.drag_position = None;
                println!("selection cancelled: surface closed");
            }
            if self.mode == Mode::Select
                && let Some(first) = self.surfaces.first()
            {
                first
                    .layer
                    .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                first.layer.commit();
            }
        }
        if self.surfaces.is_empty() {
            self.exit = true;
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        c: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(i) = self.surfaces.iter().position(|s| s.layer == *layer) {
            self.surfaces[i].size = c.new_size;
            self.draw(i, qh)
        }
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
        c: Capability,
    ) {
        if self.mode == Mode::Select {
            match c {
                Capability::Keyboard if self.keyboard.is_none() => {
                    self.keyboard = self.seats.get_keyboard(qh, &seat, None).ok()
                }
                Capability::Pointer if self.pointer.is_none() => {
                    self.pointer = self.seats.get_pointer(qh, &seat).ok()
                }
                _ => {}
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        c: Capability,
    ) {
        match c {
            Capability::Keyboard => {
                if let Some(k) = self.keyboard.take() {
                    k.release();
                }
            }
            Capability::Pointer => {
                if let Some(p) = self.pointer.take() {
                    p.release();
                }
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
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
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
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
            self.drag = None;
            self.drag_position = None;
            self.exit = true;
            println!("selection cancelled");
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
        qh: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for e in events {
            match e.kind {
                PointerEventKind::Press { button: 0x110, .. } => {
                    self.drag = Some((e.surface.clone(), e.position));
                    self.drag_position = Some(e.position);
                }
                PointerEventKind::Motion { .. } => {
                    if self
                        .drag
                        .as_ref()
                        .is_some_and(|(surface, _)| surface == &e.surface)
                    {
                        self.drag_position = Some(e.position);
                        if let Some(idx) = self
                            .surfaces
                            .iter()
                            .position(|s| s.layer.wl_surface() == &e.surface)
                        {
                            self.draw(idx, qh);
                        }
                    }
                }
                PointerEventKind::Release { button: 0x110, .. } => {
                    self.drag_position = None;
                    if let Some((surface, start)) = self.drag.take()
                        && surface == e.surface
                    {
                        let r = rectangle(start, e.position);
                        if let Some(o) = self
                            .surfaces
                            .iter()
                            .find(|o| o.layer.wl_surface() == &surface)
                        {
                            println!(
                                "selection output={} x={} y={} width={} height={}",
                                o.index, r.x, r.y, r.width, r.height
                            );
                        }
                        self.exit = true;
                    }
                }
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
wayland_client::delegate_noop!(App: ignore wl_region::WlRegion);
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_drag_in_any_direction() {
        assert_eq!(
            rectangle((30., 40.), (10., 15.)),
            Rect {
                x: 10.,
                y: 15.,
                width: 20.,
                height: 25.
            }
        );
    }
    #[test]
    fn integer_scale_and_overflow() {
        assert_eq!(buffer_dimensions((640, 480), 2), Some((1280, 960, 5120)));
        assert_eq!(buffer_dimensions((640, 480), 0), None);
        assert_eq!(buffer_dimensions((u32::MAX, 1), 2), None);
    }
    #[test]
    fn zero_sized_selection() {
        assert_eq!(rectangle((1., 2.), (1., 2.)).width, 0.);
    }
    #[test]
    fn reused_shm_slot_keeps_padding_untouched_and_covers_each_edge() {
        let (width, height) = (12usize, 10usize);
        let image_bytes = width * height * 4;
        let mut canvas = vec![0xa5; image_bytes + 64];
        paint_overlay(&mut canvas, (width as u32, height as u32), 1, 0);
        let border = 0xffd68cffu32.to_ne_bytes();
        for (x, y) in [
            (0, 0),
            (width - 1, 0),
            (0, height - 1),
            (width - 1, height - 1),
        ] {
            let offset = (y * width + x) * 4;
            assert_eq!(&canvas[offset..offset + 4], &border);
        }
        let center = ((height / 2) * width + width / 2) * 4;
        assert_eq!(&canvas[center..center + 4], &0x70301040u32.to_ne_bytes());
        assert!(canvas[image_bytes..].iter().all(|byte| *byte == 0xa5));
    }
}
