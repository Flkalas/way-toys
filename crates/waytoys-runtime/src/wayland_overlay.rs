//! Display-only per-output surfaces. No seat, keyboard or pointer is bound.
use crate::zones::Visual;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
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
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::mpsc,
    thread,
    time::Instant,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_region, wl_shm, wl_surface},
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};
use waytoys_zones_editor::render::Renderer;

enum Request {
    Visual(Option<Visual>),
    Quit,
}
pub struct Overlay {
    tx: mpsc::Sender<Request>,
    wake: UnixStream,
    thread: Option<thread::JoinHandle<()>>,
    errors: mpsc::Receiver<String>,
}
impl Overlay {
    pub fn start() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let (wake, reader) = UnixStream::pair().map_err(|e| e.to_string())?;
        reader.set_nonblocking(true).map_err(|e| e.to_string())?;
        let (ready_tx, ready_rx) = mpsc::channel();
        let (errors_tx, errors) = mpsc::channel();
        let thread = thread::spawn(move || {
            if let Err(e) = run(rx, reader, ready_tx) {
                let _ = errors_tx.send(e.to_string());
            }
        });
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())??;
        Ok(Self {
            tx,
            wake,
            thread: Some(thread),
            errors,
        })
    }
    pub fn show(&mut self, visual: Option<Visual>) {
        let _ = self.tx.send(Request::Visual(visual));
        let _ = self.wake.write_all(&[1]);
    }
    pub fn error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}
impl Drop for Overlay {
    fn drop(&mut self) {
        let _ = self.tx.send(Request::Quit);
        let _ = self.wake.write_all(&[1]);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
struct Surface {
    name: String,
    output: wl_output::WlOutput,
    layer: LayerSurface,
    pool: SlotPool,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    preferred: Option<u32>,
    integer: i32,
    size: (u32, u32),
    pending: bool,
    dirty: bool,
}
impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
    }
}
struct App {
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    layers: LayerShell,
    shm: Shm,
    viewporter: Option<WpViewporter>,
    fractional: Option<WpFractionalScaleManagerV1>,
    surfaces: Vec<Surface>,
    visual: Option<Visual>,
    renderer: Option<Renderer>,
    error: Option<String>,
}
fn run(
    rx: mpsc::Receiver<Request>,
    mut wake: UnixStream,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<(), Box<dyn Error>> {
    let setup = (|| -> Result<_, Box<dyn Error>> {
        let conn = Connection::connect_to_env()?;
        let (globals, mut queue) = registry_queue_init(&conn)?;
        let qh = queue.handle();
        let mut app = App {
            compositor: CompositorState::bind(&globals, &qh)?,
            layers: LayerShell::bind(&globals, &qh)?,
            shm: Shm::bind(&globals, &qh)?,
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            viewporter: globals.bind(&qh, 1..=1, ()).ok(),
            fractional: globals.bind(&qh, 1..=1, ()).ok(),
            surfaces: vec![],
            visual: None,
            renderer: None,
            error: None,
        };
        queue.roundtrip(&mut app)?;
        Ok((conn, queue, qh, app))
    })();
    let (conn, mut queue, qh, mut app) = match setup {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready.send(Err(e.to_string()));
            return Err(e);
        }
    };
    let mut quit = false;
    while !quit {
        queue.dispatch_pending(&mut app)?;
        let mut latest = None;
        while let Ok(request) = rx.try_recv() {
            match request {
                Request::Visual(v) => latest = Some(v),
                Request::Quit => quit = true,
            }
        }
        if quit {
            break;
        }
        if let Some(v) = latest {
            app.set_visual(v, &qh)?;
        }
        if app
            .visual
            .as_ref()
            .and_then(|v| v.label_until)
            .is_some_and(|d| Instant::now() >= d)
        {
            if let Some(v) = app.visual.as_mut() {
                v.label_until = None;
                for s in v.scenes.values_mut() {
                    s.layout_name = None;
                }
            }
            for s in &mut app.surfaces {
                s.dirty = true;
            }
        }
        for i in 0..app.surfaces.len() {
            app.draw(i, &qh)?;
        }
        if let Some(e) = app.error.take() {
            return Err(e.into());
        }
        conn.flush()?;
        if let Some(guard) = queue.prepare_read() {
            let timeout = app
                .visual
                .as_ref()
                .and_then(|v| v.label_until)
                .map(|d| Timespec::try_from(d.saturating_duration_since(Instant::now())))
                .transpose()?;
            let mut fds = [
                PollFd::new(&conn, PollFlags::IN),
                PollFd::new(&wake, PollFlags::IN),
            ];
            match poll(&mut fds, timeout.as_ref()) {
                Ok(_) => {
                    let wayland = fds[0]
                        .revents()
                        .intersects(PollFlags::IN | PollFlags::ERR | PollFlags::HUP);
                    let command = fds[1].revents().intersects(PollFlags::IN | PollFlags::HUP);
                    if wayland {
                        guard.read()?;
                    } else {
                        drop(guard);
                    }
                    if command {
                        let mut bytes = [0; 256];
                        while wake.read(&mut bytes).is_ok_and(|n| n > 0) {}
                    }
                }
                Err(rustix::io::Errno::INTR) => drop(guard),
                Err(e) => return Err(e.into()),
            }
        }
    }
    app.surfaces.clear();
    conn.flush()?;
    Ok(())
}
impl App {
    fn set_visual(
        &mut self,
        visual: Option<Visual>,
        qh: &QueueHandle<Self>,
    ) -> Result<(), Box<dyn Error>> {
        self.visual = visual;
        let Some(v) = &self.visual else {
            self.surfaces.clear();
            self.renderer = None;
            return Ok(());
        };
        self.surfaces.retain(|s| v.scenes.contains_key(&s.name));
        let names: Vec<_> = v.scenes.keys().cloned().collect();
        for name in names {
            if let Some(s) = self.surfaces.iter_mut().find(|s| s.name == name) {
                s.dirty = true;
                continue;
            }
            let Some(output) = self.outputs.outputs().find(|o| {
                self.outputs
                    .info(o)
                    .is_some_and(|i| i.name.as_deref() == Some(&name))
            }) else {
                continue;
            };
            let surface = self.compositor.create_surface(qh);
            let region = self.compositor.wl_compositor().create_region(qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();
            let viewport = self
                .viewporter
                .as_ref()
                .map(|v| v.get_viewport(&surface, qh, ()));
            let fractional = if viewport.is_some() {
                self.fractional
                    .as_ref()
                    .map(|m| m.get_fractional_scale(&surface, qh, ()))
            } else {
                None
            };
            let layer = self.layers.create_layer_surface(
                qh,
                surface,
                Layer::Overlay,
                Some("waytoys-zones-display"),
                Some(&output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_exclusive_zone(-1);
            layer.set_size(0, 0);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            let integer = self
                .outputs
                .info(&output)
                .map_or(1, |i| i.scale_factor.max(1));
            self.surfaces.push(Surface {
                name,
                output,
                layer,
                pool: SlotPool::new(4, &self.shm)?,
                viewport,
                fractional,
                preferred: None,
                integer,
                size: (0, 0),
                pending: false,
                dirty: true,
            });
        }
        if self.renderer.is_none() && !self.surfaces.is_empty() {
            self.renderer = Some(Renderer::new()?);
        }
        Ok(())
    }
    fn draw(&mut self, index: usize, qh: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        let s = &mut self.surfaces[index];
        if !s.dirty || s.pending || s.size.0 == 0 || s.size.1 == 0 {
            return Ok(());
        }
        let Some(scene) = self.visual.as_ref().and_then(|v| v.scenes.get(&s.name)) else {
            return Ok(());
        };
        let fractional = s.preferred.filter(|_| s.viewport.is_some());
        let scale = fractional.map_or(f64::from(s.integer), |v| f64::from(v) / 120.);
        let w = (f64::from(s.size.0) * scale).ceil();
        let h = (f64::from(s.size.1) * scale).ceil();
        if w <= 0. || h <= 0. || w > 16384. || h > 16384. || w * h * 4. > 256. * 1024. * 1024. {
            return Err("overlay SHM size limit".into());
        }
        let (w, h) = (w as i32, h as i32);
        let (buffer, canvas) = s
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)?;
        self.renderer
            .as_mut()
            .ok_or("overlay renderer unavailable")?
            .paint(canvas, (w as u32, h as u32), s.size, scene)?;
        if fractional.is_some() {
            s.layer.wl_surface().set_buffer_scale(1);
            s.viewport
                .as_ref()
                .unwrap()
                .set_destination(s.size.0 as i32, s.size.1 as i32);
        } else {
            s.layer.wl_surface().set_buffer_scale(s.integer);
            if let Some(v) = &s.viewport {
                v.set_destination(-1, -1);
            }
        }
        s.layer.wl_surface().damage_buffer(0, 0, w, h);
        buffer.attach_to(s.layer.wl_surface())?;
        s.layer.wl_surface().frame(qh, s.layer.wl_surface().clone());
        s.pending = true;
        s.dirty = false;
        s.layer.commit();
        Ok(())
    }
}
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        for s in &mut self.surfaces {
            if s.output == o {
                s.integer = self.outputs.info(&o).map_or(1, |i| i.scale_factor.max(1));
                s.dirty = true;
            }
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, o: wl_output::WlOutput) {
        self.surfaces.retain(|s| s.output != o);
    }
}
impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        scale: i32,
    ) {
        for s in &mut self.surfaces {
            if s.layer.wl_surface() == surface {
                s.integer = scale.max(1);
                s.dirty = true;
            }
        }
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
        for s in &mut self.surfaces {
            if s.layer.wl_surface() == surface {
                s.pending = false;
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
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.surfaces.retain(|s| s.layer != *layer);
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
            s.size = c.new_size;
            s.dirty = true;
        }
    }
}
impl Dispatch<WpFractionalScaleV1, ()> for App {
    fn event(
        state: &mut Self,
        proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && let Some(s) = state
                .surfaces
                .iter_mut()
                .find(|s| s.fractional.as_ref() == Some(proxy))
        {
            s.preferred = Some(scale);
            s.dirty = true;
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
    registry_handlers![OutputState];
}
delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_layer!(App);
delegate_registry!(App);
wayland_client::delegate_noop!(App:ignore WpViewporter);
wayland_client::delegate_noop!(App:ignore WpViewport);
wayland_client::delegate_noop!(App:ignore WpFractionalScaleManagerV1);
wayland_client::delegate_noop!(App:ignore wl_region::WlRegion);
