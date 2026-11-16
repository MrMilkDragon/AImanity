use std::{collections::HashMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Locale(usize);
const LOCALES: [(&str, &str, &str, &str); 10] = [
    ("zh-CN", "简体中文", "汉", ""),
    (
        "en",
        "English",
        "EN",
        include_str!("../../web/src/locales/en.json"),
    ),
    (
        "zh-TW",
        "繁體中文",
        "漢",
        include_str!("../../web/src/locales/zh-TW.json"),
    ),
    (
        "ja",
        "日本語",
        "日",
        include_str!("../../web/src/locales/ja.json"),
    ),
    (
        "ko",
        "한국어",
        "한",
        include_str!("../../web/src/locales/ko.json"),
    ),
    (
        "de",
        "Deutsch",
        "DE",
        include_str!("../../web/src/locales/de.json"),
    ),
    (
        "fr",
        "Français",
        "FR",
        include_str!("../../web/src/locales/fr.json"),
    ),
    (
        "es",
        "Español",
        "ES",
        include_str!("../../web/src/locales/es.json"),
    ),
    (
        "ru",
        "Русский",
        "РУ",
        include_str!("../../web/src/locales/ru.json"),
    ),
    (
        "ar",
        "العربية",
        "ع",
        include_str!("../../web/src/locales/ar.json"),
    ),
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
