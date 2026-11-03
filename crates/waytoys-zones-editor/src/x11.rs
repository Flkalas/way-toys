use crate::Options;
use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};
use waytoys_desktop::x11::{
    Display, Output, Surface,
    x11rb::{
        connection::Connection,
        protocol::{Event, xproto::KeyButMask},
    },
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

type Result<T> = std::result::Result<T, Box<dyn Error>>;
struct Editor {
    display: Arc<Display>,
    outputs: Vec<Output>,
    surface: Option<Surface>,
    options: Options,
    document: Option<CustomLayout>,
    controller: Option<Controller>,
    scene: Scene,
    preferences: PreferencesWatch,
    renderer: Renderer,
    pending: Option<usize>,
    pressed: Option<Action>,
    index: usize,
    label_until: Option<Instant>,
    exit: bool,
}
impl Editor {
    fn open(&mut self, index: usize) -> Result<()> {
        let output = self.outputs[index].clone();
        let document = match &self.options.preview {
            Some(d) => d.clone(),
            None => store::load(&self.options.config_dir, &output.name)?,
        };
        let size = Size::new(i32::from(output.width), i32::from(output.height));
        self.controller = if self.options.preview.is_none() {
            let CustomLayoutData::Grid(grid) = &document.data else {
                return Err("Grid 편집기에는 Grid 레이아웃이 필요합니다".into());
            };
            Some(Controller::new(grid.clone(), size, output.name.clone())?)
        } else {
            None
        };
        self.scene = preview::scene(&document, size, &output.name)?;
        self.document = Some(document);
        self.surface.take();
        let mut surface = Surface::new(
            self.display.clone(),
            output,
            "waytoys-zones-editor",
            true,
            true,
        )?;
        surface.show(true)?;
        self.surface = Some(surface);
        self.index = index;
        self.pending = None;
        self.pressed = None;
        self.label_until = Some(
            Instant::now()
                + Duration::from_millis(
                    self.preferences.preferences.layout_name_label_duration as u64,
                ),
        );
        self.draw()?;
        println!(
            "editor opened output={} backend=x11",
            self.outputs[index].name
        );
        Ok(())
    }
    fn save(&mut self) -> Result<()> {
        if self.options.preview.is_some() {
            return Err("preview is read-only".into());
        }
        if let (Some(c), Some(document)) = (&mut self.controller, &mut self.document) {
            let mut next = document.clone();
            next.data = CustomLayoutData::Grid(c.editor().layout().clone());
            store::save(
                &self.options.config_dir,
                &self.outputs[self.index].name,
                &next,
            )?;
            *document = next;
            c.mark_saved();
            println!(
                "saved output={} zones={}",
                self.outputs[self.index].name,
                c.editor().zones().len()
            );
        }
        Ok(())
    }
    fn action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Save => {
                self.save()?;
                self.exit = true;
            }
            Action::Cancel => self.exit = true,
            Action::NextOutput if self.outputs.len() > 1 => {
                let next = (self.index + 1) % self.outputs.len();
                if self.controller.as_ref().is_some_and(Controller::is_dirty) {
                    self.pending = Some(next);
                    self.controller.as_mut().unwrap().set_switch_prompt(true);
                } else {
                    self.open(next)?;
                }
            }
            Action::SwitchSave | Action::SwitchDiscard => {
                if action == Action::SwitchSave {
                    self.save()?;
                }
                if let Some(index) = self.pending {
                    self.open(index)?;
                }
            }
            Action::SwitchStay => {
                self.pending = None;
                if let Some(c) = &mut self.controller {
                    c.set_switch_prompt(false);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn draw(&mut self) -> Result<()> {
        let mut scene = self
            .controller
            .as_ref()
            .map_or_else(|| self.scene.clone(), Controller::scene);
        scene.preferences = self.preferences.preferences.clone();
        if self.label_until.is_some() && scene.preferences.layout_name_label_enabled {
            scene.layout_name = self.document.as_ref().map(|d| d.name.clone());
        }
        let surface = self.surface.as_ref().unwrap();
        let size = (
            u32::from(surface.output.width),
            u32::from(surface.output.height),
        );
        let mut pixels = vec![0; size.0 as usize * size.1 as usize * 4];
        self.renderer.paint(&mut pixels, size, size, &scene)?;
        surface.paint(&pixels)?;
        Ok(())
    }
    fn event(&mut self, event: Event) -> Result<()> {
        let window = self.surface.as_ref().unwrap().window;
        match event {
            Event::MotionNotify(e) if e.event == window => {
                if let Some(c) = &mut self.controller {
                    c.modifiers(e.state.contains(KeyButMask::SHIFT))?;
                    c.pointer_move(f64::from(e.event_x), f64::from(e.event_y))?;
                }
            }
            Event::ButtonPress(e) if e.event == window && e.detail == 1 => {
                if let Some(c) = &mut self.controller {
                    c.modifiers(e.state.contains(KeyButMask::SHIFT))?;
                    c.press(f64::from(e.event_x), f64::from(e.event_y))?;
                } else {
                    self.pressed = self
                        .scene
                        .buttons
                        .iter()
                        .find(|b| {
                            b.enabled && b.rect.contains(f64::from(e.event_x), f64::from(e.event_y))
                        })
                        .map(|b| b.action);
                }
            }
            Event::ButtonRelease(e) if e.event == window && e.detail == 1 => {
                let action = if let Some(c) = &mut self.controller {
                    c.release(f64::from(e.event_x), f64::from(e.event_y))?
                } else {
                    self.pressed.take().filter(|a| {
                        self.scene.buttons.iter().any(|b| {
                            b.action == *a
                                && b.rect.contains(f64::from(e.event_x), f64::from(e.event_y))
                        })
                    })
                };
                if let Some(a) = action {
                    self.action(a)?;
                }
            }
            Event::KeyPress(e) if e.event == window => {
                let key = self.display.keysym(e.detail)?;
                if key == 0xff1b {
                    self.exit = self.controller.as_mut().is_none_or(Controller::escape);
                } else if self.controller.is_some() {
                    if e.state.contains(KeyButMask::CONTROL) && matches!(key, 0x73 | 0x53) {
                        self.action(Action::Save)?;
                    } else {
                        let k = match key {
                            0xff51 => Some(EditorKey::Left),
                            0xff53 => Some(EditorKey::Right),
                            0xff52 => Some(EditorKey::Up),
                            0xff54 => Some(EditorKey::Down),
                            0x73 | 0x53 => Some(EditorKey::Split),
                            0xffff => Some(EditorKey::Delete),
                            0xff09 => Some(EditorKey::Tab),
                            _ => None,
                        };
                        let c = self.controller.as_mut().unwrap();
                        c.modifiers(
                            e.state.contains(KeyButMask::SHIFT) || matches!(key, 0xffe1 | 0xffe2),
                        )?;
                        if let Some(k) = k {
                            c.key(k)?;
                        }
                    }
                }
            }
            Event::KeyRelease(e) if e.event == window => {
                let key = self.display.keysym(e.detail)?;
                if matches!(key, 0xffe1 | 0xffe2)
                    && let Some(c) = &mut self.controller
                {
                    c.modifiers(false)?;
                }
            }
            Event::FocusOut(e) if e.event == window => {
                if let Some(c) = &mut self.controller {
                    c.pointer_lost();
                    c.modifiers(false)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
pub fn run(options: Options) -> Result<()> {
    let display = Display::open()?;
    let outputs = display.outputs()?;
    if options.list {
        let descriptions: Vec<_> = outputs
            .iter()
            .map(|o| OutputDescription {
                name: o.name.clone(),
                logical_size: Some((i32::from(o.width), i32::from(o.height))),
                logical_position: Some((i32::from(o.x), i32::from(o.y))),
                integer_scale: 1,
            })
            .collect();
        if options.json {
            println!("{}", serde_json::to_string(&descriptions)?);
        } else {
            for o in descriptions {
                println!(
                    "{}\tlogical={:?}\tposition={:?}\tinteger_scale=1",
                    o.name, o.logical_size, o.logical_position
                );
            }
        }
        return Ok(());
    }
    let index = match &options.output {
        Some(name) => outputs
            .iter()
            .position(|o| o.name == *name)
            .ok_or("selected output is unavailable")?,
        None => 0,
    };
    let mut editor = Editor {
        display,
        outputs,
        surface: None,
        preferences: PreferencesWatch::new(&options.config_dir),
        options,
        document: None,
        controller: None,
        scene: Scene::default(),
        renderer: Renderer::new()?,
        pending: None,
        pressed: None,
        index,
        label_until: None,
        exit: false,
    };
    editor.open(index)?;
    let mut check = Instant::now() + RELOAD_INTERVAL;
    while !editor.exit {
        let mut dirty = false;
        while let Some(event) = editor.display.conn.poll_for_event()? {
            if let Err(e) = editor.event(event) {
                eprintln!("editor: {e}");
                if let Some(c) = &mut editor.controller {
                    c.set_message(e.to_string());
                }
            }
            dirty = true;
        }
        let now = Instant::now();
        if now >= check {
            if editor.display.outputs()? != editor.outputs {
                return Err("화면 구성이 변경되어 편집을 취소했습니다".into());
            }
            dirty |= editor.preferences.refresh();
            check = now + RELOAD_INTERVAL;
        }
        if editor.label_until.is_some_and(|d| now >= d) {
            editor.label_until = None;
            dirty = true;
        }
        if dirty && !editor.exit {
            editor.draw()?;
        }
        editor.display.wait(Duration::from_millis(100))?;
    }
    Ok(())
}
