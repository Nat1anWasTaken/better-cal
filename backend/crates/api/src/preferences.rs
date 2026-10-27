//! Per-user preferences (stored in `users.preferences`), edited by the user themselves.

use ccal_core::problem::FieldError;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditorTheme {
    /// Follow the site's light/dark theme.
    #[default]
    Auto,
    Light,
    Dark,
    HcDark,
    HcLight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Keymap {
    #[default]
    Default,
    Vim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineNumbers {
    #[default]
    On,
    Relative,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct UserPreferences {
    pub editor_theme: EditorTheme,
    pub keymap: Keymap,
    /// 10–28 px.
    pub font_size: u8,
    /// 2, 4 or 8.
    pub tab_size: u8,
    pub word_wrap: bool,
    pub line_numbers: LineNumbers,
    pub minimap: bool,
}

impl Default for UserPreferences {
    fn default() -> Self {
        UserPreferences {
            editor_theme: EditorTheme::default(),
            keymap: Keymap::default(),
            font_size: 14,
            tab_size: 4,
            word_wrap: false,
            line_numbers: LineNumbers::default(),
            minimap: false,
        }
    }
}

impl UserPreferences {
    pub fn validate(&self) -> Vec<FieldError> {
        let mut e = vec![];
        if !(10..=28).contains(&self.font_size) {
            e.push(FieldError::new("/font_size", "out_of_range", "字體大小需為 10–28"));
        }
        if ![2, 4, 8].contains(&self.tab_size) {
            e.push(FieldError::new("/tab_size", "invalid", "縮排寬度需為 2、4 或 8"));
        }
        e
    }

    /// Reads a stored value leniently: fields dropped in later versions are
    /// ignored, and anything unreadable or out of range falls back to defaults.
    pub fn from_stored(mut v: serde_json::Value) -> Self {
        let known = serde_json::to_value(UserPreferences::default()).unwrap_or_default();
        if let (Some(m), Some(known)) = (v.as_object_mut(), known.as_object()) {
            m.retain(|k, _| known.contains_key(k));
        }
        serde_json::from_value::<UserPreferences>(v).ok().filter(|p| p.validate().is_empty()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stored_values_are_read_leniently() {
        assert_eq!(UserPreferences::from_stored(json!({})), UserPreferences::default());
        let p = UserPreferences::from_stored(json!({"keymap": "vim", "removed_field": 1}));
        assert_eq!(p.keymap, Keymap::Vim);
        assert_eq!(UserPreferences::from_stored(json!({"font_size": 99})), UserPreferences::default());
        assert_eq!(UserPreferences::from_stored(json!({"keymap": "emacs"})), UserPreferences::default());
    }
}
