use crate::{wayland_overlay, zones::Visual};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use waytoys_desktop::{
    Session,
    x11::{Display, Surface, x11rb::connection::Connection},
};
use waytoys_zones_editor::render::Renderer;

pub enum Overlay {
    Wayland(wayland_overlay::Overlay),
    X11(X11Overlay),
}
impl Overlay {
    pub fn start() -> Result<Self, String> {
        match Session::current()? {
            Session::Wayland => wayland_overlay::Overlay::start().map(Self::Wayland),
            Session::X11 => X11Overlay::start().map(Self::X11),
        }
    }
    pub fn show(&mut self, visual: Option<Visual>) {
        match self {
            Self::Wayland(o) => o.show(visual),
            Self::X11(o) => {
                let _ = o.tx.send(Some(visual));
            }
        }
    }
    pub fn error(&self) -> Option<String> {
        match self {
            Self::Wayland(o) => o.error(),
            Self::X11(o) => o.errors.try_recv().ok(),
        }
    }
}
pub struct X11Overlay {
    tx: mpsc::Sender<Option<Option<Visual>>>,
    errors: mpsc::Receiver<String>,
    thread: Option<thread::JoinHandle<()>>,
}
impl X11Overlay {
    fn start() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready) = mpsc::channel();
        let (errors_tx, errors) = mpsc::channel();
        let thread = thread::spawn(move || {
            let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                let display = Display::open()?;
                display.check_compositor()?;
                let mut renderer = Renderer::new()?;
                let _ = ready_tx.send(Ok(()));
                let mut surfaces: Vec<Surface> = Vec::new();
                let mut visual: Option<Visual> = None;
                loop {
                    let mut dirty = false;
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Ok(Some(v)) => {
                            visual = v;
                            dirty = true;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    while display.conn.poll_for_event()?.is_some() {
                        dirty = true;
                    }
                    if let Some(v) = &mut visual
                        && v.label_until.is_some_and(|t| Instant::now() >= t)
                    {
                        v.label_until = None;
                        for scene in v.scenes.values_mut() {
                            scene.layout_name = None;
                        }
                        dirty = true;
                    }
                    if !dirty {
                        continue;
                    }
                    if let Some(v) = &visual {
                        let outputs = display.outputs()?;
                        surfaces.retain(|s| {
                            v.scenes.contains_key(&s.output.name) && outputs.contains(&s.output)
                        });
                        for output in outputs {
                            let Some(scene) = v.scenes.get(&output.name) else {
                                continue;
                            };
                            let size = (u32::from(output.width), u32::from(output.height));
                            let existing = surfaces.iter().position(|s| s.output == output);
                            let index = match existing {
                                Some(index) => index,
                                None => {
                                    surfaces.push(Surface::new(
                                        display.clone(),
                                        output,
                                        "waytoys-zones-overlay",
                                        false,
                                        true,
                                    )?);
                                    surfaces.len() - 1
                                }
                            };
                            let surface = &mut surfaces[index];
                            let mut pixels = vec![0; size.0 as usize * size.1 as usize * 4];
                            renderer.paint(&mut pixels, size, size, scene)?;
                            surface.paint(&pixels)?;
                            if existing.is_none() {
                                surface.show(false)?;
                            }
                        }
                    } else {
                        surfaces.clear();
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                let message = e.to_string();
                let _ = ready_tx.send(Err(message.clone()));
                let _ = errors_tx.send(message);
            }
        });
        ready
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())??;
        Ok(Self {
            tx,
            errors,
            thread: Some(thread),
        })
    }
}
impl Drop for X11Overlay {
    fn drop(&mut self) {
        let _ = self.tx.send(None);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
