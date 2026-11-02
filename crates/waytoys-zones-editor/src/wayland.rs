//! Layer-shell adapter. All geometry supplied to the controller is surface-local
//! and logical; physical scaling is confined to SHM allocation and rendering.
use crate::Options;
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
use std::error::Error;
use std::time::{Duration, Instant};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};
use waytoys_zones::{
    geometry::Size,
    model::{CustomLayout, CustomLayoutData},
};
use waytoys_zones_config::{OutputDescription, PreferencesWatch, RELOAD_INTERVAL};
use waytoys_zones_editor::{
    controller::{Controller, EditorKey},
    preview,
    render::Renderer,
    store,
    ui::{Action, Scene},
};

struct Surface {
    output: wl_output::WlOutput,
    name: String,
    layer: LayerSurface,
    pool: SlotPool,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    preferred_scale: Option<u32>,
    integer_scale: i32,
    size: (u32, u32),
    frame_pending: bool,
    buffer_size: Option<(i32, i32)>,
}

impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(fractional) = self.fractional.take() {
            fractional.destroy();
        }
        if let Some(viewport) = self.viewport.take() {
            viewport.destroy();
        }
        // SCTK's LayerSurface owns and destroys the layer and wl_surface.
    }
}

struct App {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    layers: LayerShell,
    shm: Shm,
    viewporter: Option<WpViewporter>,
    fractional_manager: Option<WpFractionalScaleManagerV1>,
    surface: Option<Surface>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    controller: Option<Controller>,
    preview_scene: Option<Scene>,
    preview_pressed: Option<Action>,
    document: Option<CustomLayout>,
    renderer: Option<Renderer>,
    preferences: PreferencesWatch,
    label_until: Option<Instant>,
    options: Options,
    dirty: bool,
    exit: bool,
    fatal: Option<String>,
    ctrl: bool,
    pending_output: Option<String>,
}

pub fn run(options: Options) -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let mut app = App {
        compositor: CompositorState::bind(&globals, &qh)?,
        layers: LayerShell::bind(&globals, &qh)?,
        shm: Shm::bind(&globals, &qh)?,
        viewporter: globals.bind(&qh, 1..=1, ()).ok(),
        fractional_manager: globals.bind(&qh, 1..=1, ()).ok(),
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        surface: None,
        keyboard: None,
        pointer: None,
        controller: None,
        preview_scene: None,
        preview_pressed: None,
        document: None,
        renderer: if options.list {
            None
        } else {
            Some(Renderer::new()?)
        },
        preferences: PreferencesWatch::new(&options.config_dir),
        label_until: None,
        options,
        dirty: false,
        exit: false,
        fatal: None,
        ctrl: false,
        pending_output: None,
    };
    queue.roundtrip(&mut app)?;
    queue.roundtrip(&mut app)?;
    let outputs = app.named_outputs();
    if app.options.list {
        if app.options.json {
            let descriptions: Vec<_> = outputs
                .iter()
                .filter_map(|(name, output)| {
                    app.outputs.info(output).map(|info| OutputDescription {
                        name: name.clone(),
                        logical_size: info.logical_size,
                        logical_position: info.logical_position,
                        integer_scale: info.scale_factor,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string(&descriptions)?);
            return Ok(());
        }
        for (name, output) in outputs {
            if let Some(info) = app.outputs.info(&output) {
                println!(
                    "{name}\tlogical={:?}\tposition={:?}\tinteger_scale={}",
                    info.logical_size, info.logical_position, info.scale_factor
                );
            }
        }
        return Ok(());
    }
    let name = app
        .options
        .output
        .clone()
        .or_else(|| outputs.first().map(|o| o.0.clone()))
        .ok_or("no named Wayland outputs")?;
    app.open_output(&name, &qh)?;
    let mut next_check = Instant::now() + RELOAD_INTERVAL;
    let mut reported_error = None;
    while !app.exit {
        queue.dispatch_pending(&mut app)?;
        let now = Instant::now();
        if now >= next_check {
            let previous = app.preferences.preferences.clone();
            if app.preferences.refresh() && !previous.same_appearance(&app.preferences.preferences)
            {
                app.label_until = Some(
                    now + Duration::from_millis(
                        app.preferences.preferences.layout_name_label_duration as u64,
                    ),
                );
                app.dirty = true;
                println!("common preferences reloaded");
            }
            if app.preferences.error != reported_error {
                if let Some(error) = &app.preferences.error {
                    eprintln!("preferences retained: {error}");
                }
                reported_error = app.preferences.error.clone();
            }
            next_check = now + RELOAD_INTERVAL;
        }
        if app.label_until.is_some_and(|until| now >= until) {
            app.label_until = None;
            app.dirty = true;
        }
        if app.dirty && !app.exit {
            app.draw(&qh)?;
        }
        conn.flush()?;
        if !app.exit
            && let Some(guard) = queue.prepare_read()
        {
            let deadline = app.label_until.map_or(next_check, |d| d.min(next_check));
            let timeout = Timespec::try_from(deadline.saturating_duration_since(Instant::now()))?;
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
    app.surface.take();
    conn.flush()?;
    if let Some(error) = app.fatal {
        return Err(error.into());
    }
    Ok(())
}

impl App {
    fn named_outputs(&self) -> Vec<(String, wl_output::WlOutput)> {
        let mut outputs: Vec<_> = self
            .outputs
            .outputs()
            .filter_map(|o| {
                self.outputs
                    .info(&o)
                    .and_then(|info| info.name.map(|name| (name, o)))
            })
            .collect();
        outputs.sort_by(|a, b| a.0.cmp(&b.0));
        outputs
    }

    fn open_output(&mut self, name: &str, qh: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        let output = self
            .named_outputs()
            .into_iter()
            .find(|o| o.0 == name)
            .ok_or("selected output is unavailable")?
            .1;
        let document = match &self.options.preview {
            Some(layout) => layout.clone(),
            None => store::load(&self.options.config_dir, name)?,
        };
        let integer_scale = self
            .outputs
            .info(&output)
            .map_or(1, |info| info.scale_factor.max(1));
        let surface = self.compositor.create_surface(qh);
        let viewport = self
            .viewporter
            .as_ref()
            .map(|v| v.get_viewport(&surface, qh, ()));
        let fractional = if viewport.is_some() {
            self.fractional_manager
                .as_ref()
                .map(|m| m.get_fractional_scale(&surface, qh, ()))
        } else {
            None
        };
        let layer = self.layers.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("waytoys-zones-editor"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_size(0, 0);
        // Demand focus on clicking the editor, but allow capture tools and apps
        // on the other output to take focus normally.
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        let pool = SlotPool::new(4, &self.shm)?;
        self.surface.take();
        self.controller = None;
        self.preview_scene = None;
        self.preview_pressed = None;
        self.document = Some(document);
        self.pending_output = None;
        self.label_until = Some(
            Instant::now()
                + Duration::from_millis(
                    self.preferences.preferences.layout_name_label_duration as u64,
                ),
        );
        self.surface = Some(Surface {
            output,
            name: name.to_owned(),
            layer,
            pool,
            viewport,
            fractional,
            preferred_scale: None,
            integer_scale,
            size: (0, 0),
            frame_pending: false,
            buffer_size: None,
        });
        self.surface.as_ref().unwrap().layer.commit();
        self.dirty = true;
        if let Some(layout) = &self.options.preview {
            println!("preview opened output={name} layout={}", layout.name);
        } else {
            println!(
                "editor opened output={name} config={}",
                store::output_path(&self.options.config_dir, name).display()
            );
        }
        Ok(())
    }

    fn feedback(&mut self, error: impl std::fmt::Display) {
        eprintln!("editor: {error}");
        if let Some(c) = self.controller.as_mut() {
            c.set_message(error.to_string());
        }
        self.dirty = true;
    }

    fn save(&mut self) -> Result<(), Box<dyn Error>> {
        if self.options.preview.is_some() {
            return Err("preview is read-only".into());
        }
        let Some(controller) = self.controller.as_mut() else {
            return Ok(());
        };
        let Some(document) = self.document.as_mut() else {
            return Ok(());
        };
        let Some(surface) = self.surface.as_ref() else {
            return Ok(());
        };
        let mut changed = document.clone();
        changed.data = CustomLayoutData::Grid(controller.editor().layout().clone());
        store::save(&self.options.config_dir, &surface.name, &changed)?;
        *document = changed;
        controller.mark_saved();
        println!(
            "saved output={} zones={}",
            surface.name,
            controller.editor().zones().len()
        );
        Ok(())
    }

    fn action(&mut self, action: Action, qh: &QueueHandle<Self>) {
        if self.options.preview.is_some() && !matches!(action, Action::Cancel | Action::NextOutput)
        {
            return;
        }
        let result: Result<(), Box<dyn Error>> = (|| {
            match action {
                Action::Save => {
                    self.save()?;
                    self.exit = true;
                }
                Action::Cancel => self.exit = true,
                Action::NextOutput => {
                    let outputs = self.named_outputs();
                    if outputs.len() < 2 {
                        return Ok(());
                    }
                    let current = self.surface.as_ref().map(|s| s.name.as_str());
                    let idx = outputs
                        .iter()
                        .position(|o| Some(o.0.as_str()) == current)
                        .unwrap_or(0);
                    let name = outputs[(idx + 1) % outputs.len()].0.clone();
                    if self.controller.as_ref().is_some_and(Controller::is_dirty) {
                        self.pending_output = Some(name);
                        self.controller.as_mut().unwrap().set_switch_prompt(true);
                    } else {
                        self.open_output(&name, qh)?;
                    }
                }
                Action::SwitchSave | Action::SwitchDiscard => {
                    if action == Action::SwitchSave {
                        self.save()?;
                    }
                    if let Some(name) = self.pending_output.clone() {
                        self.open_output(&name, qh)?;
                    }
                }
                Action::SwitchStay => {
                    self.pending_output = None;
                    if let Some(c) = self.controller.as_mut() {
                        c.set_switch_prompt(false);
                    }
                }
                Action::Merge | Action::CancelMerge => {}
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.feedback(e);
        }
        self.dirty = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        let Some(surface) = self.surface.as_mut() else {
            return Ok(());
        };
        if surface.frame_pending || surface.size.0 == 0 || surface.size.1 == 0 {
            return Ok(());
        }
        let (width, height) = surface.size;
        let fractional = surface
            .preferred_scale
            .filter(|_| surface.viewport.is_some());
        let scale = fractional.map_or(f64::from(surface.integer_scale), |s| f64::from(s) / 120.);
        let pw = (f64::from(width) * scale).ceil();
        let ph = (f64::from(height) * scale).ceil();
        if !scale.is_finite() || scale <= 0. || pw > 16384. || ph > 16384. {
            return Err("oversized editor surface".into());
        }
        let (pw, ph) = (pw as i32, ph as i32);
        let stride = pw.checked_mul(4).ok_or("SHM stride overflow")?;
        let bytes = i64::from(stride) * i64::from(ph);
        if bytes <= 0 || bytes > 256 * 1024 * 1024 {
            return Err("editor SHM allocation exceeds 256 MiB".into());
        }
        if surface.buffer_size != Some((pw, ph)) {
            println!(
                "buffer output={} logical={}x{} physical={}x{} scale={scale}",
                surface.name, width, height, pw, ph
            );
            surface.buffer_size = Some((pw, ph));
        }
        let mut scene = match &self.controller {
            Some(controller) => controller.scene(),
            None => match &self.preview_scene {
                Some(scene) => scene.clone(),
                None => return Ok(()),
            },
        };
        scene.preferences = self.preferences.preferences.clone();
        if self.label_until.is_some() && scene.preferences.layout_name_label_enabled {
            scene.layout_name = self.document.as_ref().map(|d| d.name.clone());
        }
        let (buffer, canvas) =
            surface
                .pool
                .create_buffer(pw, ph, stride, wl_shm::Format::Argb8888)?;
        self.renderer.as_mut().expect("display renderer").paint(
            canvas,
            (pw as u32, ph as u32),
            surface.size,
            &scene,
        )?;
        if fractional.is_some() {
            surface.layer.wl_surface().set_buffer_scale(1);
            surface
                .viewport
                .as_ref()
                .unwrap()
                .set_destination(width as i32, height as i32);
        } else {
            surface
                .layer
                .wl_surface()
                .set_buffer_scale(surface.integer_scale);
            if let Some(viewport) = &surface.viewport {
                viewport.set_destination(-1, -1);
            }
        }
        surface.layer.wl_surface().damage_buffer(0, 0, pw, ph);
        buffer.attach_to(surface.layer.wl_surface())?;
        surface
            .layer
            .wl_surface()
            .frame(qh, surface.layer.wl_surface().clone());
        surface.frame_pending = true;
        surface.layer.commit();
        self.dirty = false;
        Ok(())
    }

    fn key(&mut self, event: KeyEvent, qh: &QueueHandle<Self>) {
        if self.options.preview.is_some() {
            if event.keysym == Keysym::Escape {
                self.exit = true;
            }
            return;
        }
        if self.ctrl && matches!(event.keysym, Keysym::s | Keysym::S) {
            self.action(Action::Save, qh);
            return;
        }
        if event.keysym == Keysym::Escape {
            if let Some(c) = self.controller.as_mut() {
                self.exit = c.escape();
            }
            self.dirty = true;
            return;
        }
        let key = match event.keysym {
            Keysym::Left => Some(EditorKey::Left),
            Keysym::Right => Some(EditorKey::Right),
            Keysym::Up => Some(EditorKey::Up),
            Keysym::Down => Some(EditorKey::Down),
            Keysym::s | Keysym::S => Some(EditorKey::Split),
            Keysym::Delete => Some(EditorKey::Delete),
            Keysym::Tab => Some(EditorKey::Tab),
            _ => None,
        };
        if let (Some(key), Some(c)) = (key, self.controller.as_mut()) {
            if let Err(error) = c.key(key) {
                self.feedback(error);
            }
            self.dirty = true;
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(s) = self.surface.as_mut().filter(|s| s.output == output) {
            if let Some(info) = self.outputs.info(&output) {
                s.integer_scale = info.scale_factor.max(1);
            }
            self.dirty = true;
        }
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.surface.as_ref().is_some_and(|s| s.output == output) {
            println!("editor cancelled: selected output removed");
            self.exit = true;
        }
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
        if let Some(s) = self
            .surface
            .as_mut()
            .filter(|s| s.layer.wl_surface() == surface)
        {
            s.integer_scale = scale.max(1);
            self.dirty = true;
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
        if let Some(s) = self
            .surface
            .as_mut()
            .filter(|s| s.layer.wl_surface() == surface)
        {
            s.frame_pending = false;
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
        if self.surface.as_ref().is_some_and(|s| s.layer == *layer) {
            self.exit = true;
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(s) = self.surface.as_mut().filter(|s| s.layer == *layer) else {
            return;
        };
        if configure.new_size.0 == 0 || configure.new_size.1 == 0 {
            return;
        }
        s.size = configure.new_size;
        let size = Size::new(s.size.0 as i32, s.size.1 as i32);
        let result = if self.options.preview.is_some() {
            preview::scene(self.document.as_ref().unwrap(), size, &s.name).map(|scene| {
                self.preview_scene = Some(scene);
            })
        } else if let Some(c) = self.controller.as_mut() {
            c.resize(size)
        } else {
            let CustomLayoutData::Grid(grid) = &self.document.as_ref().unwrap().data else {
                unreachable!()
            };
            Controller::new(grid.clone(), size, s.name.clone()).map(|controller| {
                self.controller = Some(controller);
            })
        };
        if let Err(error) = result {
            self.fatal = Some(error.to_string());
            self.exit = true;
        }
        println!(
            "configured output={} logical={}x{} integer_scale={}",
            s.name, s.size.0, s.size.1, s.integer_scale
        );
        self.dirty = true;
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
                .surface
                .as_mut()
                .filter(|s| s.fractional.as_ref() == Some(proxy))
        {
            s.preferred_scale = Some(scale);
            state.dirty = true;
            println!("fractional scale output={} scale={}/120", s.name, scale);
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
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard if self.keyboard.is_none() => {
                self.keyboard = self.seats.get_keyboard(qh, &seat, None).ok()
            }
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seats.get_pointer(qh, &seat).ok()
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard => {
                if let Some(k) = self.keyboard.take() {
                    k.release();
                }
                self.ctrl = false;
                if let Some(c) = self.controller.as_mut() {
                    let _ = c.modifiers(false);
                }
            }
            Capability::Pointer => {
                if let Some(p) = self.pointer.take() {
                    p.release();
                }
                if let Some(c) = self.controller.as_mut() {
                    c.pointer_lost();
                }
                self.preview_pressed = None;
            }
            _ => {}
        }
        self.dirty = true;
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {
        if let Some(k) = self.keyboard.take() {
            k.release();
        }
        if let Some(p) = self.pointer.take() {
            p.release();
        }
        self.ctrl = false;
        self.preview_pressed = None;
        if let Some(c) = self.controller.as_mut() {
            c.pointer_lost();
            let _ = c.modifiers(false);
        }
        self.dirty = true;
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
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.ctrl = false;
        if let Some(c) = self.controller.as_mut() {
            let _ = c.modifiers(false);
        }
        self.dirty = true;
    }
    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event, qh);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event, qh);
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
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.ctrl = modifiers.ctrl;
        if let Some(c) = self.controller.as_mut()
            && let Err(error) = c.modifiers(modifiers.shift)
        {
            self.feedback(error);
        }
        self.dirty = true;
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
        for event in events {
            if !self
                .surface
                .as_ref()
                .is_some_and(|s| s.layer.wl_surface() == &event.surface)
            {
                continue;
            }
            let (x, y) = event.position;
            if let Some(scene) = self.preview_scene.as_ref() {
                let hit = scene
                    .buttons
                    .iter()
                    .rev()
                    .find(|b| b.enabled && b.rect.contains(x, y))
                    .map(|b| b.action);
                match event.kind {
                    PointerEventKind::Press { button: 0x110, .. } => self.preview_pressed = hit,
                    PointerEventKind::Release { button: 0x110, .. } => {
                        if let Some(action) =
                            self.preview_pressed.take().filter(|a| Some(*a) == hit)
                        {
                            self.action(action, qh);
                        }
                    }
                    PointerEventKind::Leave { .. } => self.preview_pressed = None,
                    _ => {}
                }
                continue;
            }
            let Some(c) = self.controller.as_mut() else {
                continue;
            };
            if matches!(
                event.kind,
                PointerEventKind::Press { .. } | PointerEventKind::Release { .. }
            ) {
                println!(
                    "pointer output={} x={x:.2} y={y:.2} event={:?}",
                    self.surface.as_ref().unwrap().name,
                    event.kind
                );
            }
            let result = match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    c.pointer_move(x, y).map(|_| None)
                }
                PointerEventKind::Press { button: 0x110, .. } => c.press(x, y).map(|_| None),
                PointerEventKind::Release { button: 0x110, .. } => c.release(x, y),
                PointerEventKind::Leave { .. } => {
                    c.pointer_lost();
                    Ok(None)
                }
                _ => Ok(None),
            };
            match result {
                Ok(Some(action)) => self.action(action, qh),
                Err(error) => self.feedback(error),
                _ => {}
            }
            if matches!(event.kind, PointerEventKind::Release { button: 0x110, .. })
                && let Some(c) = &self.controller
            {
                println!(
                    "edit zones={} dirty={}",
                    c.editor().zones().len(),
                    c.is_dirty()
                );
            }
            self.dirty = true;
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
wayland_client::delegate_noop!(App: ignore WpFractionalScaleManagerV1);
