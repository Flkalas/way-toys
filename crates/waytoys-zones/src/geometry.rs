//! Work-area-local logical geometry. Render-buffer pixels are a separate unit.

use serde::{Deserialize, Serialize};

use crate::{Result, ZonesError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }

    pub fn validate(self) -> Result<()> {
        if self.width <= 0 || self.height <= 0 {
            return Err(ZonesError::InvalidData(
                "work area must have positive dimensions".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    pub fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    pub fn validate(self) -> Result<()> {
        if self.width <= 0
            || self.height <= 0
            || self.right() > i64::from(i32::MAX)
            || self.bottom() > i64::from(i32::MAX)
        {
            return Err(ZonesError::InvalidData(
                "rectangle is empty or exceeds coordinate range".into(),
            ));
        }
        Ok(())
    }

    pub fn contains(self, point: Point) -> bool {
        point.is_finite()
            && point.x >= f64::from(self.x)
            && point.y >= f64::from(self.y)
            && point.x < self.right() as f64
            && point.y < self.bottom() as f64
    }

    pub fn area(self) -> i64 {
        i64::from(self.width) * i64::from(self.height)
    }

    pub fn union(self, other: Self) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let width = self.right().max(other.right()) - i64::from(x);
        let height = self.bottom().max(other.bottom()) - i64::from(y);
        let result = Self::new(
            x,
            y,
            i32::try_from(width)
                .map_err(|_| ZonesError::InvalidData("rectangle union width overflow".into()))?,
            i32::try_from(height)
                .map_err(|_| ZonesError::InvalidData("rectangle union height overflow".into()))?,
        );
        result.validate()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_rectangles_have_one_owner_at_the_shared_edge() {
        let a = Rect::new(0, 0, 100, 80);
        let b = Rect::new(100, 0, 100, 80);
        let p = Point::new(100.0, 20.0);
        assert!(!a.contains(p));
        assert!(b.contains(p));
    }

    #[test]
    fn invalid_geometry_is_rejected_without_integer_overflow() {
        assert!(Rect::new(i32::MAX, 0, 1, 1).validate().is_err());
        assert!(Rect::new(0, 0, 0, 10).validate().is_err());
        assert!(!Rect::new(0, 0, 10, 10).contains(Point::new(f64::NAN, 1.0)));
        assert!(
            Rect::new(i32::MIN, 0, 1, 1)
                .union(Rect::new(0, 0, 1, 1))
                .is_err()
        );
    }
}
