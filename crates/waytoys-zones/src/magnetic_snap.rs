// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license. See LICENSE-MIT-PowerToys in the repository root.
// Rust adaptation of PowerToys 8827f848fbbac4afeedcae2907c71cd05fd5aa86:
// src/modules/fancyzones/editor/FancyZonesEditor/MagneticSnap.cs.

//! Grid-editor snapping in the source's 10,000-unit coordinate space.
//!
//! This is deliberately distinct from canvas snapping: the grid maximum magnet
//! width is `10000 / 12`, whereas the canvas uses 8% of its work-area axis.
//! Inputs are validated and key points are sorted/deduplicated at construction;
//! the C# caller supplies already sorted prefix sums.

use crate::model::GRID_MULTIPLIER;
use crate::{Result, ZonesError};

#[derive(Clone, Debug)]
pub struct MagneticSnap {
    key_points: Vec<i32>,
    work_area_size: f64,
}

impl MagneticSnap {
    pub fn new(mut key_points: Vec<i32>, work_area_size: f64) -> Result<Self> {
        if !work_area_size.is_finite() || work_area_size <= 0.0 {
            return Err(ZonesError::InvalidData(
                "snap work-area size must be finite and positive".into(),
            ));
        }
        if key_points
            .iter()
            .any(|point| !(0..=GRID_MULTIPLIER).contains(point))
        {
            return Err(ZonesError::InvalidData(
                "grid snap points must be within 0..=10000".into(),
            ));
        }
        key_points.sort_unstable();
        key_points.dedup();
        Ok(Self {
            key_points,
            work_area_size,
        })
    }

    /// Convert a work-area-local coordinate to normalized data, then attract it
    /// to interior key points. The outer bounds are clamped, not magnets.
    ///
    /// `Convert.ToInt32(double)` rounds halfway values to even in the source;
    /// Rust's usual `round()` would incorrectly round them away from zero.
    pub fn pixel_to_data_with_snapping(&self, pixel: f64, low: i32, high: i32) -> Result<i32> {
        if low < 0 || low > high || high > GRID_MULTIPLIER {
            return Err(ZonesError::InvalidData(
                "grid snap bounds must satisfy 0 <= low <= high <= 10000".into(),
            ));
        }
        let rounded = (pixel / self.work_area_size * f64::from(GRID_MULTIPLIER)).round_ties_even();
        if !rounded.is_finite() || rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
            return Err(ZonesError::InvalidData(
                "snap coordinate is nonfinite or outside the integer data range".into(),
            ));
        }
        let data = (rounded as i32).clamp(low, high);
        let points: Vec<i32> = self
            .key_points
            .iter()
            .copied()
            .filter(|point| low < *point && *point < high)
            .collect();
        for (index, &point) in points.iter().enumerate() {
            let previous = if index == 0 { low } else { points[index - 1] };
            let next = points.get(index + 1).copied().unwrap_or(high);
            let radius = (point - previous)
                .min(next - point)
                .min(GRID_MULTIPLIER / 12)
                / 2;
            if (data - point).abs() <= radius {
                return Ok(
                    (attract(i64::from(data), i64::from(point), i64::from(radius)) as i32)
                        .clamp(low, high),
                );
            }
        }
        Ok(data)
    }

    pub fn data_to_pixel_without_snapping(&self, data: i32) -> f64 {
        self.work_area_size * f64::from(data) / f64::from(GRID_MULTIPLIER)
    }
}

/// Shared source transfer curve. Call only after the coordinate is inside the
/// magnet radius; wider arithmetic avoids overflows from hostile drag deltas.
pub(crate) fn attract(position: i64, point: i64, radius: i64) -> i64 {
    let dead_zone_width = (radius + 1) / 2;
    if (position - point).abs() <= dead_zone_width {
        point
    } else if position < point {
        position + (position - (point - radius))
    } else {
        position - ((point + radius) - position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_dead_zone_and_outer_attraction_curve() {
        let snap = MagneticSnap::new(vec![5000], 1000.0).unwrap();
        // Max radius = (10000 / 12) / 2 = 416; dead zone = 208.
        assert_eq!(
            snap.pixel_to_data_with_snapping(480.0, 0, 10000).unwrap(),
            5000
        );
        assert_eq!(
            snap.pixel_to_data_with_snapping(470.0, 0, 10000).unwrap(),
            4816
        );
        assert_eq!(
            snap.pixel_to_data_with_snapping(530.0, 0, 10000).unwrap(),
            5184
        );
        assert_eq!(
            snap.pixel_to_data_with_snapping(450.0, 0, 10000).unwrap(),
            4500
        );
    }

    #[test]
    fn close_points_and_bounds_shrink_the_magnet_radius() {
        let snap = MagneticSnap::new(vec![5100, 5000, 5000], 10000.0).unwrap();
        assert_eq!(
            snap.pixel_to_data_with_snapping(4975.0, 0, 10000).unwrap(),
            5000
        );
        assert_eq!(
            snap.pixel_to_data_with_snapping(4960.0, 0, 10000).unwrap(),
            4970
        );
        // A point at a bound is not eligible for attraction.
        assert_eq!(
            snap.pixel_to_data_with_snapping(4990.0, 0, 5000).unwrap(),
            4990
        );
        assert_eq!(snap.pixel_to_data_with_snapping(-1.0, 0, 5000).unwrap(), 0);
        assert_eq!(
            snap.pixel_to_data_with_snapping(10001.0, 0, 5000).unwrap(),
            5000
        );
        assert_eq!(snap.pixel_to_data_with_snapping(123.0, 50, 50).unwrap(), 50);
    }

    #[test]
    fn source_ties_even_and_coordinate_roundtrip() {
        // These binary-exact quotients remain exact half-integers after
        // multiplying by 10000, exercising both directions of ties-to-even.
        let snap = MagneticSnap::new(vec![], 32.0).unwrap();
        assert_eq!(
            snap.pixel_to_data_with_snapping(1.0, 0, 10000).unwrap(),
            312
        );
        assert_eq!(
            snap.pixel_to_data_with_snapping(3.0, 0, 10000).unwrap(),
            938
        );
        assert_eq!(
            snap.pixel_to_data_with_snapping(5.0, 0, 10000).unwrap(),
            1562
        );
        let snap = MagneticSnap::new(vec![], 10000.0).unwrap();
        assert_eq!(snap.pixel_to_data_with_snapping(0.5, 0, 10000).unwrap(), 0);
        // The source evaluates division before multiplication: 1.5 / 10000
        // * 10000 is 1.4999999999999998, not a half-integer. Convert.ToInt32
        // consequently returns 1. Do not simplify away this evaluation order.
        assert_eq!(snap.pixel_to_data_with_snapping(1.5, 0, 10000).unwrap(), 1);
        assert_eq!(snap.pixel_to_data_with_snapping(2.5, 0, 10000).unwrap(), 2);
        let snap = MagneticSnap::new(vec![], 1920.0).unwrap();
        for data in [0, 1, 3333, 5000, 9999, 10000] {
            let pixel = snap.data_to_pixel_without_snapping(data);
            assert_eq!(
                snap.pixel_to_data_with_snapping(pixel, 0, 10000).unwrap(),
                data
            );
        }
    }

    #[test]
    fn invalid_numeric_input_is_rejected() {
        for size in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(MagneticSnap::new(vec![], size).is_err());
        }
        assert!(MagneticSnap::new(vec![10001], 1000.0).is_err());
        let snap = MagneticSnap::new(vec![], 1000.0).unwrap();
        for pixel in [f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(snap.pixel_to_data_with_snapping(pixel, 0, 10000).is_err());
        }
        assert!(snap.pixel_to_data_with_snapping(0.0, 1, 0).is_err());
    }
}
