//! Software rendering for native-endian, premultiplied ARGB8888 SHM buffers.

use crate::ui::{RectF, Scene};
use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::process::Command;
use waytoys_zones_config::{LayoutNameLabelPlacement as Placement, parse_color};

type Color = [u8; 4]; // Straight RGBA at the drawing API; SHM pixels are premultiplied.

const TEXT: Color = [244, 248, 255, 255];
const BLUE: Color = [83, 173, 255, 240];

struct Glyph {
    metrics: Metrics,
    coverage: Vec<u8>,
}

pub struct Renderer {
    font: Font,
    // The key includes the actual rasterization size, including fractional scale.
    glyphs: HashMap<(char, u32), Glyph>,
}

impl Renderer {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        // Font discovery and loading happen once, never in the frame loop.
        let output = Command::new("fc-match")
            .args(["-f", "%{file}\n%{index}\n", "sans-serif"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other("fc-match could not find a sans-serif font").into());
        }
        let description = std::str::from_utf8(&output.stdout)?;
        let mut lines = description.lines();
        let path = lines
            .next()
            .filter(|line| !line.is_empty())
            .ok_or_else(|| io::Error::other("fc-match returned no font path"))?;
        // Fontconfig encodes a named variable-font instance in the upper bits.
        // fontdue accepts the underlying collection face index.
        let collection_index = lines.next().unwrap_or("0").parse::<u32>()? & 0xffff;
        let font = Font::from_bytes(
            std::fs::read(path)?,
            FontSettings {
                collection_index,
                ..FontSettings::default()
            },
        )
        .map_err(io::Error::other)?;
        Ok(Self {
            font,
            glyphs: HashMap::new(),
        })
    }

    pub fn paint(
        &mut self,
        canvas: &mut [u8],
        physical: (u32, u32),
        logical: (u32, u32),
        scene: &Scene,
    ) -> Result<(), Box<dyn Error>> {
        let mut painter = Painter::new(canvas, physical, logical)?;
        painter.clear(if scene.runtime_overlay {
            [0, 0, 0, 0]
        } else {
            [14, 20, 31, 100]
        });
        let preferences = &scene.preferences;
        let color = |value: &str, alpha| {
            let [r, g, b] = parse_color(value).unwrap_or([0, 0, 0]);
            [r, g, b, alpha]
        };
        let opacity = (preferences.zone_highlight_opacity.clamp(0, 100) * 255 / 100) as u8;
        for zone in &scene.zones {
            painter.fill(
                zone.rect,
                if zone.selected {
                    color(&preferences.zone_highlight_color, opacity)
                } else {
                    color(&preferences.zone_color, opacity)
                },
            );
            painter.stroke(zone.rect, 1.5, color(&preferences.zone_border_color, 255));
            if valid_rect(zone.rect) {
                let cx = zone.rect.x + zone.rect.width / 2.0;
                let cy = zone.rect.y + zone.rect.height / 2.0;
                let id = zone.id.saturating_add(1).to_string();
                if preferences.show_zone_number {
                    self.centered_text(
                        &mut painter,
                        cx,
                        cy - 25.0,
                        30.0,
                        &id,
                        color(&preferences.zone_number_color, 255),
                        zone.rect,
                    );
                }
                if !scene.runtime_overlay {
                    let size = format!("{:.0} x {:.0}", zone.rect.width, zone.rect.height);
                    self.centered_text(&mut painter, cx, cy + 13.0, 13.0, &size, TEXT, zone.rect);
                }
            }
        }
        if let Some(name) = &scene.layout_name {
            let size = preferences.layout_name_label_font_size as f32;
            if let Some((width, height)) = self.text_dimensions(&painter, size, name) {
                // Reserve the toolbar and status strip in this editor surface.
                let bounds = painter.surface();
                let surface = if scene.runtime_overlay {
                    bounds
                } else {
                    RectF::new(
                        8.0,
                        112.0,
                        (bounds.width - 16.0).max(0.0),
                        (bounds.height - 160.0).max(0.0),
                    )
                };
                let pad = f64::from(preferences.layout_name_label_padding);
                let width = (width + 2.0 * pad).min(surface.width);
                let height = (height + 2.0 * pad).min(surface.height);
                let x = match preferences.layout_name_label_placement {
                    Placement::TopLeft | Placement::BottomLeft => surface.x,
                    Placement::TopRight | Placement::BottomRight => {
                        surface.x + surface.width - width
                    }
                    _ => surface.x + (surface.width - width) / 2.0,
                };
                let y = match preferences.layout_name_label_placement {
                    Placement::BottomLeft | Placement::BottomCenter | Placement::BottomRight => {
                        surface.y + surface.height - height
                    }
                    Placement::Center => surface.y + (surface.height - height) / 2.0,
                    _ => surface.y,
                };
                let rect = RectF::new(x, y, width, height);
                painter.fill(
                    rect,
                    color(&preferences.layout_name_label_background_color, 255),
                );
                self.text(
                    &mut painter,
                    x + pad,
                    y + pad,
                    size,
                    name,
                    color(&preferences.layout_name_label_text_color, 255),
                    rect,
                );
            }
        }
        for divider in &scene.dividers {
            painter.fill(
                divider.rect,
                if divider.active {
                    [255, 192, 84, 250]
                } else {
                    [113, 194, 255, 225]
                },
            );
        }
        if let Some((rect, valid)) = scene.split {
            let color = if valid { BLUE } else { [159, 166, 178, 190] };
            painter.fill(rect, [color[0], color[1], color[2], 90]);
            painter.stroke(rect, 2.0, color);
        }
        if let Some(rect) = scene.selection {
            painter.fill(rect, [74, 160, 245, 35]);
            painter.stroke(rect, 2.0, BLUE);
        }

        // The modal scrim dims the zones while its own controls stay fully legible.
        if scene.modal {
            painter.fill(painter.surface(), [0, 0, 0, 165]);
        }
        for label in &scene.labels {
            let clip = painter.surface();
            if let Some((width, height)) = self.text_dimensions(&painter, label.size, &label.text) {
                painter.fill(
                    RectF::new(label.x - 6.0, label.y - 4.0, width + 12.0, height + 8.0),
                    [19, 27, 40, 242],
                );
                self.text(
                    &mut painter,
                    label.x,
                    label.y,
                    label.size,
                    &label.text,
                    TEXT,
                    clip,
                );
            }
        }
        for button in &scene.buttons {
            painter.fill(
                button.rect,
                if button.enabled {
                    [31, 49, 72, 250]
                } else {
                    [36, 40, 48, 245]
                },
            );
            painter.stroke(
                button.rect,
                1.0,
                if button.enabled {
                    [125, 168, 215, 235]
                } else {
                    [88, 96, 108, 180]
                },
            );
            if let Some((_, height)) = self.text_dimensions(&painter, 14.0, &button.text) {
                self.centered_text(
                    &mut painter,
                    button.rect.x + button.rect.width / 2.0,
                    button.rect.y + (button.rect.height - height) / 2.0,
                    14.0,
                    &button.text,
                    if button.enabled {
                        TEXT
                    } else {
                        [155, 163, 177, 255]
                    },
                    button.rect,
                );
            }
        }
        Ok(())
    }

    fn raster_size(painter: &Painter<'_>, size: f32) -> Option<f32> {
        let pixels = (f64::from(size) * painter.sy) as f32;
        // Bound untrusted font sizes before fontdue allocates a raster bitmap.
        (size.is_finite() && size > 0.0 && pixels.is_finite() && pixels > 0.0 && pixels <= 512.0)
            .then_some(pixels)
    }

    fn glyph(&mut self, character: char, pixels: f32) -> &Glyph {
        self.glyphs
            .entry((character, pixels.to_bits()))
            .or_insert_with(|| {
                let (metrics, coverage) = self.font.rasterize(character, pixels);
                Glyph { metrics, coverage }
            })
    }

    fn text_dimensions(
        &mut self,
        painter: &Painter<'_>,
        size: f32,
        text: &str,
    ) -> Option<(f64, f64)> {
        let pixels = Self::raster_size(painter, size)?;
        let width = text
            .chars()
            .map(|c| f64::from(self.glyph(c, pixels).metrics.advance_width))
            .sum::<f64>()
            / painter.sy;
        let height = self
            .font
            .horizontal_line_metrics(pixels)
            .map(|m| f64::from(m.ascent - m.descent) / painter.sy)
            .unwrap_or(f64::from(size));
        (width.is_finite() && height.is_finite()).then_some((width, height))
    }

    #[allow(clippy::too_many_arguments)]
    fn centered_text(
        &mut self,
        painter: &mut Painter<'_>,
        x: f64,
        y: f64,
        size: f32,
        text: &str,
        color: Color,
        clip: RectF,
    ) {
        if let Some((width, _)) = self.text_dimensions(painter, size, text) {
            self.text(painter, x - width / 2.0, y, size, text, color, clip);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        painter: &mut Painter<'_>,
        mut x: f64,
        y: f64,
        size: f32,
        text: &str,
        color: Color,
        clip: RectF,
    ) {
        if !x.is_finite() || !y.is_finite() || !valid_rect(clip) {
            return;
        }
        let Some(pixels) = Self::raster_size(painter, size) else {
            return;
        };
        let ascent = self
            .font
            .horizontal_line_metrics(pixels)
            .map(|m| f64::from(m.ascent) / painter.sy)
            .unwrap_or(f64::from(size));
        let baseline = y + ascent;
        for character in text.chars() {
            let glyph = self.glyph(character, pixels);
            let rect = RectF::new(
                x + f64::from(glyph.metrics.xmin) / painter.sy,
                baseline
                    - (f64::from(glyph.metrics.ymin) + glyph.metrics.height as f64) / painter.sy,
                glyph.metrics.width as f64 / painter.sy,
                glyph.metrics.height as f64 / painter.sy,
            );
            painter.bitmap(rect, clip, glyph, color);
            x += f64::from(glyph.metrics.advance_width) / painter.sy;
            if !x.is_finite() {
                break;
            }
        }
    }
}

fn valid_rect(rect: RectF) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width > 0.0
        && rect.height > 0.0
        && (rect.x + rect.width).is_finite()
        && (rect.y + rect.height).is_finite()
}

fn intersection(a: RectF, b: RectF) -> Option<RectF> {
    if !valid_rect(a) || !valid_rect(b) {
        return None;
    }
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(RectF::new(x, y, right - x, bottom - y))
}

struct Painter<'a> {
    canvas: &'a mut [u8],
    width: usize,
    height: usize,
    logical: (u32, u32),
    sx: f64,
    sy: f64,
}

impl<'a> Painter<'a> {
    fn new(
        canvas: &'a mut [u8],
        physical: (u32, u32),
        logical: (u32, u32),
    ) -> Result<Self, io::Error> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid SHM buffer dimensions or length",
            )
        };
        if physical.0 == 0 || physical.1 == 0 || logical.0 == 0 || logical.1 == 0 {
            return Err(invalid());
        }
        let width = usize::try_from(physical.0).map_err(|_| invalid())?;
        let height = usize::try_from(physical.1).map_err(|_| invalid())?;
        let length = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(invalid)?;
        if canvas.len() < length {
            return Err(invalid());
        }
        Ok(Self {
            canvas: &mut canvas[..length],
            width,
            height,
            logical,
            sx: f64::from(physical.0) / f64::from(logical.0),
            sy: f64::from(physical.1) / f64::from(logical.1),
        })
    }

    fn surface(&self) -> RectF {
        RectF::new(
            0.0,
            0.0,
            f64::from(self.logical.0),
            f64::from(self.logical.1),
        )
    }

    fn clear(&mut self, color: Color) {
        let pixel = premultiply(color).to_ne_bytes();
        for target in self.canvas.as_chunks_mut::<4>().0 {
            target.copy_from_slice(&pixel);
        }
    }

    fn bounds(&self, rect: RectF) -> Option<(usize, usize, usize, usize)> {
        // Clip in logical space before scaling, so enormous finite inputs cannot overflow.
        let clipped = intersection(rect, self.surface())?;
        let x0 = (clipped.x * self.sx).floor() as usize;
        let y0 = (clipped.y * self.sy).floor() as usize;
        let x1 = ((clipped.x + clipped.width) * self.sx).ceil() as usize;
        let y1 = ((clipped.y + clipped.height) * self.sy).ceil() as usize;
        Some((
            x0.min(self.width),
            y0.min(self.height),
            x1.min(self.width),
            y1.min(self.height),
        ))
    }

    fn fill(&mut self, rect: RectF, color: Color) {
        let Some((x0, y0, x1, y1)) = self.bounds(rect) else {
            return;
        };
        for y in y0..y1 {
            for x in x0..x1 {
                self.pixel(x, y, color);
            }
        }
    }

    fn stroke(&mut self, rect: RectF, width: f64, color: Color) {
        if !valid_rect(rect) || !width.is_finite() || width <= 0.0 {
            return;
        }
        let w = width.min(rect.width / 2.0).min(rect.height / 2.0);
        self.fill(RectF::new(rect.x, rect.y, rect.width, w), color);
        self.fill(
            RectF::new(rect.x, rect.y + rect.height - w, rect.width, w),
            color,
        );
        self.fill(
            RectF::new(rect.x, rect.y + w, w, rect.height - 2.0 * w),
            color,
        );
        self.fill(
            RectF::new(
                rect.x + rect.width - w,
                rect.y + w,
                w,
                rect.height - 2.0 * w,
            ),
            color,
        );
    }

    fn bitmap(&mut self, rect: RectF, clip: RectF, glyph: &Glyph, color: Color) {
        let Some(clipped) = intersection(rect, clip) else {
            return;
        };
        let Some((x0, y0, x1, y1)) = self.bounds(clipped) else {
            return;
        };
        for y in y0..y1 {
            let ly = (y as f64 + 0.5) / self.sy;
            if ly < clipped.y || ly >= clipped.y + clipped.height {
                continue;
            }
            let row = (((ly - rect.y) / rect.height) * glyph.metrics.height as f64) as usize;
            if row >= glyph.metrics.height {
                continue;
            }
            for x in x0..x1 {
                let lx = (x as f64 + 0.5) / self.sx;
                if lx < clipped.x || lx >= clipped.x + clipped.width {
                    continue;
                }
                let column = (((lx - rect.x) / rect.width) * glyph.metrics.width as f64) as usize;
                if column >= glyph.metrics.width {
                    continue;
                }
                let coverage = glyph.coverage[row * glyph.metrics.width + column];
                let alpha = multiply(color[3], coverage);
                if alpha != 0 {
                    self.pixel(x, y, [color[0], color[1], color[2], alpha]);
                }
            }
        }
    }

    fn pixel(&mut self, x: usize, y: usize, color: Color) {
        let offset = (y * self.width + x) * 4;
        let target = &mut self.canvas[offset..offset + 4];
        let destination = u32::from_ne_bytes(target.try_into().expect("four-byte pixel"));
        let source = premultiply(color);
        let inverse_alpha = 255 - color[3];
        let mut result = 0;
        for shift in [0, 8, 16, 24] {
            let src = ((source >> shift) & 255) as u8;
            let dst = ((destination >> shift) & 255) as u8;
            let channel = u16::from(src) + u16::from(multiply(dst, inverse_alpha));
            result |= u32::from(channel.min(255)) << shift;
        }
        target.copy_from_slice(&result.to_ne_bytes());
    }
}

fn multiply(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}

fn premultiply([r, g, b, a]: Color) -> u32 {
    (u32::from(a) << 24)
        | (u32::from(multiply(r, a)) << 16)
        | (u32::from(multiply(g, a)) << 8)
        | u32::from(multiply(b, a))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(canvas: &[u8], width: usize, x: usize, y: usize) -> u32 {
        u32::from_ne_bytes(
            canvas[(y * width + x) * 4..(y * width + x + 1) * 4]
                .try_into()
                .unwrap(),
        )
    }

    #[test]
    fn clears_only_the_image_and_preserves_slot_padding() {
        let mut canvas = [0xa5; 31];
        let mut painter = Painter::new(&mut canvas, (3, 2), (3, 2)).unwrap();
        painter.clear([100, 50, 20, 128]);
        assert_eq!(pixel(&canvas, 3, 2, 1), premultiply([100, 50, 20, 128]));
        assert_eq!(&canvas[24..], &[0xa5; 7]);
    }

    #[test]
    fn premultiplied_source_over_uses_native_argb_order() {
        let mut canvas = [0; 4];
        let mut painter = Painter::new(&mut canvas, (1, 1), (1, 1)).unwrap();
        painter.clear([0, 0, 255, 128]);
        painter.fill(painter.surface(), [255, 0, 0, 128]);
        assert_eq!(canvas, 0xc0800040_u32.to_ne_bytes());
    }

    #[test]
    fn clips_negative_enormous_and_nonfinite_rectangles() {
        let mut canvas = [0; 4 * 4 * 4];
        let mut painter = Painter::new(&mut canvas, (4, 4), (4, 4)).unwrap();
        painter.fill(RectF::new(-100.0, -100.0, 102.0, 102.0), [255; 4]);
        painter.fill(RectF::new(f64::NAN, 0.0, 4.0, 4.0), [0, 255, 0, 255]);
        painter.fill(RectF::new(0.0, f64::INFINITY, 4.0, 4.0), [0, 255, 0, 255]);
        assert_eq!(pixel(&canvas, 4, 1, 1), u32::MAX);
        assert_eq!(pixel(&canvas, 4, 2, 1), 0);
        let mut painter = Painter::new(&mut canvas, (4, 4), (4, 4)).unwrap();
        painter.fill(RectF::new(0.0, 0.0, 1e300, 1e300), [7, 9, 11, 255]);
        assert_eq!(pixel(&canvas, 4, 3, 3), 0xff07090b);
    }

    #[test]
    fn maps_each_axis_using_the_actual_fractional_buffer_ratio() {
        for physical in [(10, 8), (13, 11)] {
            let mut canvas = vec![0; physical.0 as usize * physical.1 as usize * 4];
            let mut painter = Painter::new(&mut canvas, physical, (8, 6)).unwrap();
            painter.fill(RectF::new(4.0, 3.0, 4.0, 3.0), [255; 4]);
            let start_x = (4.0 * f64::from(physical.0) / 8.0).floor() as usize;
            let start_y = (3.0 * f64::from(physical.1) / 6.0).floor() as usize;
            for y in 0..physical.1 as usize {
                for x in 0..physical.0 as usize {
                    assert_eq!(
                        pixel(&canvas, physical.0 as usize, x, y),
                        if x >= start_x && y >= start_y {
                            u32::MAX
                        } else {
                            0
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn clips_glyph_coverage_and_respects_independent_scale_axes() {
        let glyph = Glyph {
            metrics: Metrics {
                width: 2,
                height: 2,
                ..Metrics::default()
            },
            coverage: vec![255, 128, 0, 255],
        };
        let mut canvas = [0; 8 * 6 * 4];
        let mut painter = Painter::new(&mut canvas, (8, 6), (4, 4)).unwrap();
        painter.bitmap(
            RectF::new(-1.0, 0.0, 2.0, 2.0),
            painter.surface(),
            &glyph,
            [255; 4],
        );
        assert_eq!(pixel(&canvas, 8, 0, 0), 0x80808080);
        assert_eq!(pixel(&canvas, 8, 1, 2), u32::MAX);
        assert_eq!(pixel(&canvas, 8, 2, 0), 0);
        assert_eq!(pixel(&canvas, 8, 0, 3), 0);
    }

    #[test]
    fn rejects_zero_short_and_overflowing_buffers_without_writing() {
        let mut canvas = [0xa5; 12];
        for (physical, logical) in [
            ((0, 2), (2, 2)),
            ((2, 2), (0, 2)),
            ((2, 2), (2, 2)),
            ((u32::MAX, u32::MAX), (1, 1)),
        ] {
            assert!(Painter::new(&mut canvas, physical, logical).is_err());
            assert_eq!(canvas, [0xa5; 12]);
        }
    }
}
