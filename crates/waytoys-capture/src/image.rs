//! Logical desktop coordinates are converted only here, at the image boundary.
use std::io::Cursor;

pub const MAX_PIXELS: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub fn right(self) -> f64 {
        self.x as f64 + self.width as f64
    }
    pub fn bottom(self) -> f64 {
        self.y as f64 + self.height as f64
    }
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x as f64 && x < self.right() && y >= self.y as f64 && y < self.bottom()
    }
    pub fn intersects(self, other: Self) -> bool {
        (self.x as f64) < other.right()
            && (other.x as f64) < self.right()
            && (self.y as f64) < other.bottom()
            && (other.y as f64) < self.bottom()
    }
}

pub fn selection(a: (f64, f64), b: (f64, f64), bounds: Rect) -> Option<Rect> {
    if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) || a.0 == b.0 || a.1 == b.1 {
        return None;
    }
    let x = a.0.min(b.0).floor().max(bounds.x as f64);
    let y = a.1.min(b.1).floor().max(bounds.y as f64);
    let right = a.0.max(b.0).ceil().min(bounds.right());
    let bottom = a.1.max(b.1).ceil().min(bounds.bottom());
    (right > x && bottom > y).then_some(Rect {
        x: x as i32,
        y: y as i32,
        width: (right - x) as u32,
        height: (bottom - y) as u32,
    })
}

#[derive(Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Native-endian, straight-alpha ARGB32. KWin desktops are normally opaque.
    pub pixels: Vec<u32>,
}
impl Image {
    pub fn png(&self) -> Result<Vec<u8>, String> {
        let mut rgba = Vec::with_capacity(self.pixels.len() * 4);
        for &p in &self.pixels {
            rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
        }
        let mut out = Cursor::new(Vec::new());
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    }
}

pub struct Screen {
    pub name: String,
    pub rect: Rect,
    pub image: Image,
    pub dimmed: Vec<u32>,
}
impl Screen {
    pub fn new(name: String, rect: Rect, image: Image) -> Self {
        let dimmed = image
            .pixels
            .iter()
            .map(|p| 0xff000000 | ((p & 0x00fefefe) >> 1))
            .collect();
        Self {
            name,
            rect,
            image,
            dimmed,
        }
    }
    fn crop_bounds(&self, r: Rect) -> (usize, usize, usize, usize) {
        let sx = self.image.width as f64 / self.rect.width as f64;
        let sy = self.image.height as f64 / self.rect.height as f64;
        let x = ((r.x as f64 - self.rect.x as f64) * sx)
            .floor()
            .clamp(0.0, self.image.width as f64) as usize;
        let y = ((r.y as f64 - self.rect.y as f64) * sy)
            .floor()
            .clamp(0.0, self.image.height as f64) as usize;
        let right = ((r.right() - self.rect.x as f64) * sx)
            .ceil()
            .clamp(0.0, self.image.width as f64) as usize;
        let bottom = ((r.bottom() - self.rect.y as f64) * sy)
            .ceil()
            .clamp(0.0, self.image.height as f64) as usize;
        (x, y, right, bottom)
    }
    pub fn paint(&self, canvas: &mut [u8], region: Option<Rect>) {
        let pixels: &mut [u32] = bytemuck::cast_slice_mut(&mut canvas[..self.dimmed.len() * 4]);
        pixels.copy_from_slice(&self.dimmed);
        if let Some(r) = region.filter(|r| self.rect.intersects(*r)) {
            let (x, y, right, bottom) = self.crop_bounds(r);
            let width = self.image.width as usize;
            for row in y..bottom {
                pixels[row * width + x..row * width + right]
                    .copy_from_slice(&self.image.pixels[row * width + x..row * width + right]);
            }
            // Only actual selection edges are outlined, not monitor seams.
            for row in y..bottom {
                if r.x >= self.rect.x && x < right {
                    pixels[row * width + x] = 0xffffffff;
                }
                if r.right() <= self.rect.right() && x < right {
                    pixels[row * width + right - 1] = 0xffffffff;
                }
            }
            if y < bottom {
                if r.y >= self.rect.y {
                    pixels[y * width + x..y * width + right].fill(0xffffffff);
                }
                if r.bottom() <= self.rect.bottom() {
                    pixels[(bottom - 1) * width + x..(bottom - 1) * width + right].fill(0xffffffff);
                }
            }
        }
    }
}

pub fn compose(screens: &[Screen], region: Rect) -> Result<Image, String> {
    let relevant: Vec<_> = screens
        .iter()
        .filter(|s| s.rect.intersects(region))
        .collect();
    if relevant.is_empty() {
        return Err("선택한 영역에 화면이 없습니다".into());
    }
    if relevant.len() == 1 {
        let s = relevant[0];
        // A crop wholly within one screen is copied without any resampling.
        if region.x >= s.rect.x
            && region.y >= s.rect.y
            && region.right() <= s.rect.right()
            && region.bottom() <= s.rect.bottom()
        {
            let (x, y, right, bottom) = s.crop_bounds(region);
            let mut pixels = Vec::with_capacity((right - x) * (bottom - y));
            for row in y..bottom {
                pixels.extend_from_slice(
                    &s.image.pixels
                        [row * s.image.width as usize + x..row * s.image.width as usize + right],
                );
            }
            return Ok(Image {
                width: (right - x) as u32,
                height: (bottom - y) as u32,
                pixels,
            });
        }
    }
    // A mixed-DPI rectangle uses the densest participating output; desktop gaps
    // remain transparent. Pixel centers avoid cumulative monitor-edge drift.
    let scale = relevant
        .iter()
        .map(|s| {
            (s.image.width as f64 / s.rect.width as f64)
                .max(s.image.height as f64 / s.rect.height as f64)
        })
        .fold(1.0, f64::max);
    let width = (region.width as f64 * scale).ceil() as u32;
    let height = (region.height as f64 * scale).ceil() as u32;
    let count = (width as usize)
        .checked_mul(height as usize)
        .filter(|n| *n <= MAX_PIXELS)
        .ok_or("선택한 이미지가 너무 큽니다")?;
    let mut pixels = vec![0; count];
    for y in 0..height {
        let gy = region.y as f64 + (y as f64 + 0.5) * region.height as f64 / height as f64;
        for x in 0..width {
            let gx = region.x as f64 + (x as f64 + 0.5) * region.width as f64 / width as f64;
            if let Some(s) = relevant.iter().find(|s| s.rect.contains(gx, gy)) {
                let px =
                    ((gx - s.rect.x as f64) * s.image.width as f64 / s.rect.width as f64) as usize;
                let py = ((gy - s.rect.y as f64) * s.image.height as f64 / s.rect.height as f64)
                    as usize;
                pixels[y as usize * width as usize + x as usize] =
                    s.image.pixels[py.min(s.image.height as usize - 1) * s.image.width as usize
                        + px.min(s.image.width as usize - 1)];
            }
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen(x: i32, w: u32, physical: u32, color: u32) -> Screen {
        Screen::new(
            "test".into(),
            Rect {
                x,
                y: 0,
                width: w,
                height: 10,
            },
            Image {
                width: physical,
                height: 10,
                pixels: vec![color; (physical * 10) as usize],
            },
        )
    }
    #[test]
    fn selection_reverses_clips_and_rejects_empty_or_nonfinite_input() {
        let bounds = Rect {
            x: -100,
            y: -50,
            width: 300,
            height: 150,
        };
        assert_eq!(
            selection((90.2, 90.5), (-200.0, -90.0), bounds),
            Some(Rect {
                x: -100,
                y: -50,
                width: 191,
                height: 141
            })
        );
        assert!(selection((1.0, 1.0), (1.0, 8.0), bounds).is_none());
        assert!(selection((f64::NAN, 1.0), (7.0, 8.0), bounds).is_none());
    }
    #[test]
    fn fractional_single_output_crop_preserves_original_pixels() {
        let mut s = screen(-10, 10, 15, 0);
        s.image.pixels = (0..150).collect();
        let image = compose(
            &[s],
            Rect {
                x: -8,
                y: 2,
                width: 3,
                height: 2,
            },
        )
        .unwrap();
        assert_eq!((image.width, image.height), (5, 2));
        assert_eq!(image.pixels, vec![33, 34, 35, 36, 37, 48, 49, 50, 51, 52]);
    }
    #[test]
    fn mixed_outputs_use_highest_density_and_keep_gaps_transparent() {
        let image = compose(
            &[
                screen(-10, 10, 10, 0xffff0000),
                screen(2, 10, 20, 0xff00ff00),
            ],
            Rect {
                x: -2,
                y: 0,
                width: 6,
                height: 4,
            },
        )
        .unwrap();
        assert_eq!((image.width, image.height), (12, 8));
        assert_eq!(
            &image.pixels[..12],
            &[
                0xffff0000, 0xffff0000, 0xffff0000, 0xffff0000, 0, 0, 0, 0, 0xff00ff00, 0xff00ff00,
                0xff00ff00, 0xff00ff00
            ]
        );
    }
    #[test]
    fn painting_does_not_alter_source_and_png_has_straight_rgba() {
        let s = screen(0, 10, 10, 0xff804020);
        let mut buffer = vec![0u32; 104];
        s.paint(
            bytemuck::cast_slice_mut(&mut buffer),
            Some(Rect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            }),
        );
        assert_eq!(buffer[0], 0xff402010);
        assert_eq!(buffer[22], 0xffffffff);
        assert_eq!(buffer[33], 0xff804020);
        assert_eq!(buffer[100], 0);
        let png = s.image.png().unwrap();
        let mut decoder = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut bytes = vec![0; decoder.output_buffer_size().unwrap()];
        decoder.next_frame(&mut bytes).unwrap();
        assert_eq!(&bytes[..4], &[0x80, 0x40, 0x20, 0xff]);
    }
}
