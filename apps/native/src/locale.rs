use std::{collections::HashMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Locale(usize);
const LOCALES: [(&str, &str, &str, &str); 10] = [
    ("zh-CN", "简体中文", "汉", ""),
    ("en", "English", "EN", include_str!("../locales/en.json")),
    (
        "zh-TW",
        "繁體中文",
        "漢",
        include_str!("../locales/zh-TW.json"),
    ),
    ("ja", "日本語", "日", include_str!("../locales/ja.json")),
    ("ko", "한국어", "한", include_str!("../locales/ko.json")),
    ("de", "Deutsch", "DE", include_str!("../locales/de.json")),
    ("fr", "Français", "FR", include_str!("../locales/fr.json")),
    ("es", "Español", "ES", include_str!("../locales/es.json")),
    ("ru", "Русский", "РУ", include_str!("../locales/ru.json")),
    ("ar", "العربية", "ع", include_str!("../locales/ar.json")),
];
impl Locale {
    pub fn all() -> impl Iterator<Item = Self> {
        (0..LOCALES.len()).map(Self)
    }
    pub fn parse(code: &str) -> Self {
        Self(
            LOCALES
                .iter()
                .position(|item| item.0 == code)
                .unwrap_or_default(),
        )
    }
    pub fn code(self) -> &'static str {
        LOCALES[self.0].0
    }
    pub fn name(self) -> &'static str {
        LOCALES[self.0].1
    }
    pub fn glyph(self) -> &'static str {
        LOCALES[self.0].2
    }
    pub fn tr(self, key: &str) -> &str {
        static CATALOGS: [OnceLock<HashMap<String, String>>; LOCALES.len()] =
            [const { OnceLock::new() }; LOCALES.len()];
        if self.0 == 0 {
            return key;
        }
        CATALOGS[self.0]
            .get_or_init(|| {
                serde_json::from_str(LOCALES[self.0].3).expect("bundled translation catalog")
            })
            .get(key)
            .map_or(key, String::as_str)
    }
}
