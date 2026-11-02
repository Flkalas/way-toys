use std::collections::BTreeMap;
use waytoys_zones_config::ZonesPreferences;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Opacity,
    Highlight,
    Inactive,
    Border,
    Number,
    TextColor,
    Background,
    FontSize,
    Padding,
    Duration,
}
impl Field {
    pub const ALL: [Self; 10] = [
        Self::Opacity,
        Self::Highlight,
        Self::Inactive,
        Self::Border,
        Self::Number,
        Self::TextColor,
        Self::Background,
        Self::FontSize,
        Self::Padding,
        Self::Duration,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Opacity => "불투명도 (%) · 0–100",
            Self::Highlight => "강조색",
            Self::Inactive => "비활성색",
            Self::Border => "테두리색",
            Self::Number => "번호색",
            Self::TextColor => "글자색",
            Self::Background => "배경색",
            Self::FontSize => "글자 크기 · 8–128",
            Self::Padding => "여백 · 0–64",
            Self::Duration => "표시 시간 (ms) · 300–10000",
        }
    }
    pub fn is_color(self) -> bool {
        !matches!(
            self,
            Self::Opacity | Self::FontSize | Self::Padding | Self::Duration
        )
    }
    pub fn value(self, p: &ZonesPreferences) -> String {
        match self {
            Self::Opacity => p.zone_highlight_opacity.to_string(),
            Self::Highlight => p.zone_highlight_color.clone(),
            Self::Inactive => p.zone_color.clone(),
            Self::Border => p.zone_border_color.clone(),
            Self::Number => p.zone_number_color.clone(),
            Self::TextColor => p.layout_name_label_text_color.clone(),
            Self::Background => p.layout_name_label_background_color.clone(),
            Self::FontSize => p.layout_name_label_font_size.to_string(),
            Self::Padding => p.layout_name_label_padding.to_string(),
            Self::Duration => p.layout_name_label_duration.to_string(),
        }
    }
    fn assign(self, value: &str, p: &mut ZonesPreferences) -> Result<(), String> {
        let integer = || {
            value
                .parse::<i32>()
                .map_err(|_| "숫자를 끝까지 입력해 주세요".to_owned())
        };
        match self {
            Self::Opacity => p.zone_highlight_opacity = integer()?,
            Self::Highlight => p.zone_highlight_color = value.into(),
            Self::Inactive => p.zone_color = value.into(),
            Self::Border => p.zone_border_color = value.into(),
            Self::Number => p.zone_number_color = value.into(),
            Self::TextColor => p.layout_name_label_text_color = value.into(),
            Self::Background => p.layout_name_label_background_color = value.into(),
            Self::FontSize => p.layout_name_label_font_size = integer()?,
            Self::Padding => p.layout_name_label_padding = integer()?,
            Self::Duration => p.layout_name_label_duration = integer()?,
        }
        p.validate().map_err(|e| match e {
            waytoys_zones::ZonesError::InvalidData(message) => message,
            other => other.to_string(),
        })
    }
}

pub struct Draft {
    pub values: BTreeMap<Field, String>,
    pub errors: BTreeMap<Field, String>,
}
impl Draft {
    pub fn new(p: &ZonesPreferences) -> Self {
        Self {
            values: Field::ALL.into_iter().map(|f| (f, f.value(p))).collect(),
            errors: BTreeMap::new(),
        }
    }
    /// Incomplete input changes only the draft, never the persisted model.
    pub fn edit(
        &mut self,
        f: Field,
        value: String,
        p: &ZonesPreferences,
    ) -> Option<ZonesPreferences> {
        let mut next = p.clone();
        let result = f.assign(&value, &mut next);
        self.values.insert(f, value);
        match result {
            Ok(()) => {
                self.errors.remove(&f);
                Some(next)
            }
            Err(e) => {
                self.errors.insert(f, e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_inputs_keep_the_last_valid_model_and_other_drafts() {
        let mut p = ZonesPreferences::default();
        let mut draft = Draft::new(&p);
        assert!(draft.edit(Field::Highlight, "#00".into(), &p).is_none());
        assert_eq!(p.zone_highlight_color, "#008CFF");
        p = draft.edit(Field::Opacity, "72".into(), &p).unwrap();
        assert_eq!(p.zone_highlight_opacity, 72);
        assert!(draft.errors.contains_key(&Field::Highlight));
        for bad in ["", "-", "101", "1.5"] {
            assert!(draft.edit(Field::Opacity, bad.into(), &p).is_none());
        }
        assert_eq!(p.zone_highlight_opacity, 72);
        p = draft.edit(Field::Highlight, "#ABCdef".into(), &p).unwrap();
        assert_eq!(p.zone_highlight_color, "#ABCdef");
    }
}
