//! Read-only custom-layout scenes in output-local logical coordinates.

use waytoys_zones::geometry::Size;
use waytoys_zones::layout::calculate_custom;
use waytoys_zones::model::{CustomLayout, CustomLayoutData};

use crate::ui::{Action, Button, Label, RectF, Scene, ZoneVisual};

pub fn scene(layout: &CustomLayout, size: Size, output_name: &str) -> waytoys_zones::Result<Scene> {
    layout.validate()?;
    let zones = calculate_custom(layout, size)?
        .into_iter()
        .map(|zone| ZoneVisual {
            rect: RectF::new(
                f64::from(zone.rect.x),
                f64::from(zone.rect.y),
                f64::from(zone.rect.width),
                f64::from(zone.rect.height),
            ),
            id: zone.id,
            selected: false,
        })
        .collect();
    let kind = match &layout.data {
        CustomLayoutData::Canvas(_) => "Canvas",
        CustomLayoutData::Grid(_) => "Grid",
    };
    Ok(Scene {
        zones,
        buttons: vec![
            Button {
                rect: RectF::new(16.0, 12.0, 150.0, 36.0),
                text: "Next output".into(),
                action: Action::NextOutput,
                enabled: true,
            },
            Button {
                rect: RectF::new((f64::from(size.width) - 116.0).max(16.0), 12.0, 100.0, 36.0),
                text: "Close".into(),
                action: Action::Cancel,
                enabled: true,
            },
        ],
        labels: vec![
            Label {
                x: 20.0,
                y: 64.0,
                size: 22.0,
                text: format!("{kind} preview: {}", layout.name),
            },
            Label {
                x: 20.0,
                y: 96.0,
                size: 14.0,
                text: "Read-only | Overlaps are intentional | Esc: close".into(),
            },
            Label {
                x: 20.0,
                y: (f64::from(size.height) - 28.0).max(120.0),
                size: 13.0,
                text: format!("{output_name} | {} x {} logical", size.width, size.height),
            },
        ],
        ..Scene::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use waytoys_zones::geometry::Rect;
    use waytoys_zones::model::{CustomLayouts, GridLayout};

    fn fixture() -> CustomLayout {
        let mut layouts =
            CustomLayouts::from_json(include_str!("../examples/canvas-overlap.json")).unwrap();
        assert_eq!(layouts.custom_layouts.len(), 1);
        layouts.custom_layouts.remove(0)
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        i64::from(a.x) < b.right()
            && i64::from(b.x) < a.right()
            && i64::from(a.y) < b.bottom()
            && i64::from(b.y) < a.bottom()
    }

    #[test]
    fn example_retains_overlaps_and_nested_zone() {
        let layout = fixture();
        assert_eq!(layout.name, "Overlapping Canvas");
        let CustomLayoutData::Canvas(canvas) = &layout.data else {
            panic!("expected Canvas example");
        };
        assert_eq!((canvas.ref_width, canvas.ref_height), (1280, 720));
        let zones = calculate_custom(&layout, Size::new(1280, 720)).unwrap();
        assert_eq!(zones.len(), 4);
        assert!(overlaps(zones[0].rect, zones[1].rect));
        assert!(overlaps(zones[1].rect, zones[2].rect));
        let (outer, inner) = (zones[0].rect, zones[3].rect);
        assert!(inner.x > outer.x && inner.y > outer.y);
        assert!(inner.right() < outer.right() && inner.bottom() < outer.bottom());
    }

    #[test]
    fn scene_uses_calculated_local_rectangles_and_only_navigation_controls() {
        let layout = fixture();
        let size = Size::new(1113, 626);
        let calculated = calculate_custom(&layout, size).unwrap();
        let preview = scene(&layout, size, "DP-1").unwrap();
        assert_eq!(preview.zones.len(), 4);
        for (visual, zone) in preview.zones.iter().zip(&calculated) {
            assert_eq!(visual.id, zone.id);
            assert!(!visual.selected);
            assert_eq!(
                visual.rect,
                RectF::new(
                    f64::from(zone.rect.x),
                    f64::from(zone.rect.y),
                    f64::from(zone.rect.width),
                    f64::from(zone.rect.height),
                )
            );
            assert!(visual.rect.x >= 0.0 && visual.rect.y >= 0.0);
            assert!(visual.rect.x + visual.rect.width <= f64::from(size.width));
            assert!(visual.rect.y + visual.rect.height <= f64::from(size.height));
        }
        assert_eq!(preview.zones[0].rect, RectF::new(69.0, 121.0, 539.0, 391.0));
        assert!(preview.dividers.is_empty());
        assert!(preview.split.is_none());
        assert!(preview.selection.is_none());
        assert!(!preview.modal);
        assert_eq!(
            preview
                .buttons
                .iter()
                .map(|button| button.action)
                .collect::<Vec<_>>(),
            vec![Action::NextOutput, Action::Cancel]
        );
        assert_eq!(preview.buttons[1].text, "Close");
        assert_eq!(preview.labels[0].text, "Canvas preview: Overlapping Canvas");
        assert!(preview.labels[2].text.contains("DP-1"));
    }

    #[test]
    fn toolbar_fits_848_logical_pixels_and_labels_follow_it() {
        let preview = scene(&fixture(), Size::new(848, 480), "DP-1").unwrap();
        assert!(
            preview.buttons[0].rect.x + preview.buttons[0].rect.width < preview.buttons[1].rect.x
        );
        assert!(preview.buttons[1].rect.x + preview.buttons[1].rect.width <= 848.0);
        assert!(preview.labels[0].y > 48.0);
        assert!(preview.labels[1].y > preview.labels[0].y);
    }

    #[test]
    fn grid_preview_uses_the_same_read_only_path() {
        let layout = CustomLayout::new(
            "grid-example",
            "Single zone",
            CustomLayoutData::Grid(GridLayout::default()),
        )
        .unwrap();
        let preview = scene(&layout, Size::new(848, 480), "DP-1").unwrap();
        assert_eq!(preview.zones.len(), 1);
        assert_eq!(preview.zones[0].rect, RectF::new(16.0, 16.0, 816.0, 448.0));
        assert!(preview.dividers.is_empty());
    }

    #[test]
    fn invalid_surface_size_is_rejected() {
        for size in [Size::new(0, 720), Size::new(1280, 0), Size::new(-1, 720)] {
            assert!(scene(&fixture(), size, "DP-1").is_err());
        }
    }
}
