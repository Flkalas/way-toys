use crate::image::{self, Image, Rect, Screen};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use waytoys_desktop::x11::{
    Display, Surface,
    x11rb::{
        connection::Connection,
        protocol::{Event, xproto::NotifyMode},
    },
};

pub fn run(check: bool, stop: Arc<AtomicBool>) -> Result<Option<Image>, Box<dyn Error>> {
    let display = Display::open()?;
    let outputs = display.outputs()?;
    let count: usize = outputs
        .iter()
        .map(|o| usize::from(o.width) * usize::from(o.height))
        .sum();
    if count > image::MAX_PIXELS {
        return Err("전체 화면 이미지가 너무 큽니다".into());
    }
    let mut screens = Vec::new();
    for output in &outputs {
        let pixels = display.screenshot(output)?;
        println!(
            "captured output={} x11={},{} {}x{}",
            output.name, output.x, output.y, output.width, output.height
        );
        screens.push(Screen::new(
            output.name.clone(),
            Rect {
                x: i32::from(output.x),
                y: i32::from(output.y),
                width: u32::from(output.width),
                height: u32::from(output.height),
            },
            Image {
                width: u32::from(output.width),
                height: u32::from(output.height),
                pixels,
            },
        ));
    }
    if check {
        return Ok(None);
    }
    let x = screens.iter().map(|s| s.rect.x).min().unwrap();
    let y = screens.iter().map(|s| s.rect.y).min().unwrap();
    let right = screens.iter().map(|s| s.rect.right() as i32).max().unwrap();
    let bottom = screens
        .iter()
        .map(|s| s.rect.bottom() as i32)
        .max()
        .unwrap();
    let bounds = Rect {
        x,
        y,
        width: (right - x) as u32,
        height: (bottom - y) as u32,
    };
    let mut surfaces = Vec::new();
    for (i, output) in outputs.iter().enumerate() {
        let mut surface = Surface::new(
            display.clone(),
            output.clone(),
            "waytoys-capture",
            true,
            false,
        )?;
        let mut pixels = vec![0; screens[i].image.pixels.len() * 4];
        screens[i].paint(&mut pixels, None);
        surface.paint(&pixels)?;
        surface.show(i == 0)?;
        surfaces.push(surface);
    }
    surfaces[0].grab()?;
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, stop.clone())?;
    }
    let mut anchor = None;
    let mut selected = None;
    let mut dirty = false;
    let mut next_check = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        while let Some(event) = display.conn.poll_for_event()? {
            match event {
                Event::KeyPress(e) if display.keysym(e.detail)? == 0xff1b => return Ok(None),
                Event::ButtonPress(e) if e.detail == 3 => return Ok(None),
                Event::ButtonPress(e) if e.detail == 1 => {
                    anchor = Some((f64::from(e.root_x), f64::from(e.root_y)));
                    selected = None;
                    dirty = true;
                }
                Event::MotionNotify(e) => {
                    if let Some(a) = anchor {
                        selected =
                            image::selection(a, (f64::from(e.root_x), f64::from(e.root_y)), bounds);
                        dirty = true;
                    }
                }
                Event::ButtonRelease(e) if e.detail == 1 => {
                    if let Some(a) = anchor.take() {
                        selected =
                            image::selection(a, (f64::from(e.root_x), f64::from(e.root_y)), bounds);
                        if let Some(rect) = selected {
                            return Ok(Some(image::compose(&screens, rect)?));
                        }
                    }
                }
                Event::Expose(_) => dirty = true,
                Event::FocusOut(e)
                    if e.event == surfaces[0].window && e.mode == NotifyMode::NORMAL =>
                {
                    return Ok(None);
                }
                _ => {}
            }
        }
        if Instant::now() >= next_check {
            if display.outputs()? != outputs {
                return Ok(None);
            }
            next_check = Instant::now() + Duration::from_millis(250);
        }
        if dirty {
            for (surface, screen) in surfaces.iter().zip(&screens) {
                let mut pixels = vec![0; screen.image.pixels.len() * 4];
                screen.paint(&mut pixels, selected);
                surface.paint(&pixels)?;
            }
            dirty = false;
        }
        display.wait(Duration::from_millis(250))?;
    }
    Ok(None)
}
