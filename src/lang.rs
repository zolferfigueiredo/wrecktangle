use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(windows)]
use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
#[cfg(windows)]
use windows::core::PWSTR;

pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    catalog: &'static str,
}

pub const LANGUAGES: [Language; 12] = [
    Language {
        code: "de",
        name: "Deutsch",
        catalog: include_str!("../lang/de.json"),
    },
    Language {
        code: "en",
        name: "English",
        catalog: include_str!("../lang/en.json"),
    },
    Language {
        code: "es",
        name: "Español",
        catalog: include_str!("../lang/es.json"),
    },
    Language {
        code: "fr",
        name: "Français",
        catalog: include_str!("../lang/fr.json"),
    },
    Language {
        code: "it",
        name: "Italiano",
        catalog: include_str!("../lang/it.json"),
    },
    Language {
        code: "pl",
        name: "Polski",
        catalog: include_str!("../lang/pl.json"),
    },
    Language {
        code: "pt",
        name: "Português",
        catalog: include_str!("../lang/pt.json"),
    },
    Language {
        code: "ru",
        name: "Русский",
        catalog: include_str!("../lang/ru.json"),
    },
    Language {
        code: "uk",
        name: "Українська",
        catalog: include_str!("../lang/uk.json"),
    },
    Language {
        code: "zh",
        name: "中文",
        catalog: include_str!("../lang/zh.json"),
    },
    Language {
        code: "ja",
        name: "日本語",
        catalog: include_str!("../lang/ja.json"),
    },
    Language {
        code: "ko",
        name: "한국어",
        catalog: include_str!("../lang/ko.json"),
    },
];

const ENGLISH: usize = 1;

static CURRENT: AtomicUsize = AtomicUsize::new(ENGLISH);
static TABLES: OnceLock<Vec<HashMap<String, String>>> = OnceLock::new();

fn tables() -> &'static [HashMap<String, String>] {
    TABLES.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|language| {
                serde_json::from_str(language.catalog).expect("every catalog is valid JSON")
            })
            .collect()
    })
}

pub fn index_of(code: &str) -> Option<usize> {
    LANGUAGES.iter().position(|language| language.code == code)
}

pub fn is_supported(code: &str) -> bool {
    index_of(code).is_some()
}

pub fn set(code: &str) -> bool {
    match index_of(code) {
        Some(index) => {
            CURRENT.store(index, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn current_index() -> usize {
    CURRENT.load(Ordering::Relaxed)
}

// Han characters have no locale hint in the renderer's fallback, so Chinese
// text would come out of a Japanese font and miss simplified glyphs.
pub fn ui_font() -> &'static str {
    match LANGUAGES[current_index()].code {
        "zh" => "Microsoft YaHei UI",
        "ja" => "Yu Gothic UI",
        "ko" => "Malgun Gothic",
        _ => "",
    }
}

pub fn t(key: &str) -> String {
    lookup(current_index(), key)
}

#[cfg(test)]
pub fn t_in(code: &str, key: &str) -> String {
    lookup(index_of(code).unwrap_or(ENGLISH), key)
}

fn lookup(index: usize, key: &str) -> String {
    let tables = tables();
    tables[index]
        .get(key)
        .or_else(|| tables[ENGLISH].get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

pub fn format(key: &str, vars: &[(&str, &str)]) -> String {
    fill(t(key), vars)
}

#[cfg(test)]
pub fn format_in(code: &str, key: &str, vars: &[(&str, &str)]) -> String {
    fill(t_in(code, key), vars)
}

fn fill(mut text: String, vars: &[(&str, &str)]) -> String {
    for (name, value) in vars {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

pub fn pick_supported<S: AsRef<str>>(tags: &[S]) -> &'static str {
    tags.iter()
        .filter_map(|tag| tag.as_ref().get(..2))
        .find_map(|prefix| {
            let prefix = prefix.to_ascii_lowercase();
            LANGUAGES
                .iter()
                .find(|language| language.code == prefix)
                .map(|language| language.code)
        })
        .unwrap_or("en")
}

#[cfg(windows)]
pub fn detect_system() -> &'static str {
    pick_supported(&preferred_ui_languages())
}

#[cfg(not(windows))]
pub fn detect_system() -> &'static str {
    "en"
}

#[cfg(windows)]
fn preferred_ui_languages() -> Vec<String> {
    unsafe {
        let mut count = 0u32;
        let mut len = 0u32;
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).is_err()
            || len == 0
        {
            return Vec::new();
        }
        let mut buffer = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut len,
        )
        .is_err()
        {
            return Vec::new();
        }
        buffer
            .split(|&unit| unit == 0)
            .filter(|tag| !tag.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}') else {
                break;
            };
            let name = &rest[start..start + end + 1];
            if name[1..name.len() - 1]
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_')
                && !found.contains(&name.to_string())
            {
                found.push(name.to_string());
            }
            rest = &rest[start + end + 1..];
        }
        found.sort();
        found
    }

    fn catalog(code: &str) -> &'static HashMap<String, String> {
        &tables()[index_of(code).expect("language exists")]
    }

    #[test]
    fn there_are_twelve_languages_with_unique_codes_and_names() {
        assert_eq!(LANGUAGES.len(), 12);
        let mut codes: Vec<_> = LANGUAGES.iter().map(|l| l.code).collect();
        let mut names: Vec<_> = LANGUAGES.iter().map(|l| l.name).collect();
        codes.sort_unstable();
        codes.dedup();
        names.sort_unstable();
        names.dedup();
        assert_eq!(codes.len(), 12);
        assert_eq!(names.len(), 12);
        assert!(LANGUAGES.iter().all(|l| l.code.len() == 2));
        assert_eq!(LANGUAGES[ENGLISH].code, "en");
    }

    #[test]
    fn every_language_has_every_english_key_and_no_extras() {
        let english = catalog("en");
        for language in &LANGUAGES {
            let table = catalog(language.code);
            for key in english.keys() {
                assert!(table.contains_key(key), "{}: missing {key}", language.code);
            }
            for key in table.keys() {
                assert!(
                    english.contains_key(key),
                    "{}: extra {key} not in English",
                    language.code
                );
            }
        }
    }

    #[test]
    fn placeholders_match_english() {
        let english = catalog("en");
        for language in &LANGUAGES {
            for (key, value) in catalog(language.code) {
                assert_eq!(
                    placeholders(value),
                    placeholders(&english[key]),
                    "{}: {key}",
                    language.code
                );
            }
        }
    }

    #[test]
    fn no_value_is_empty_or_has_dashes_or_three_dots() {
        for language in &LANGUAGES {
            for (key, value) in catalog(language.code) {
                assert!(
                    !value.trim().is_empty(),
                    "{}: {key} is empty",
                    language.code
                );
                assert!(
                    !value.contains('\u{2014}') && !value.contains('\u{2013}'),
                    "{}: {key} has a dash",
                    language.code
                );
                assert!(
                    !value.contains("..."),
                    "{}: {key} has three dots",
                    language.code
                );
            }
        }
    }

    #[test]
    fn catalog_files_have_no_dashes_and_keys_in_sorted_order() {
        for language in &LANGUAGES {
            assert!(
                !language.catalog.contains('\u{2014}') && !language.catalog.contains('\u{2013}'),
                "{} has a dash",
                language.code
            );
            let keys: Vec<&str> = language
                .catalog
                .lines()
                .filter_map(|line| line.strip_prefix("  \""))
                .filter_map(|line| line.split_once("\": "))
                .map(|(key, _)| key)
                .collect();
            assert!(!keys.is_empty());
            assert!(
                keys.windows(2).all(|pair| pair[0] < pair[1]),
                "{} keys are not sorted",
                language.code
            );
        }
    }

    #[test]
    fn every_action_and_owner_key_exists() {
        let english = catalog("en");
        for action in crate::layout::Action::ALL {
            assert!(english.contains_key(action.key()), "{}", action.key());
        }
        assert!(english.keys().filter(|k| k.starts_with("owner.")).count() >= 40);
    }

    #[test]
    fn lookup_falls_back_to_english_and_then_the_key() {
        assert_eq!(t_in("de", "tab.about"), "Info");
        assert_eq!(t_in("xx", "tab.about"), "About");
        assert_eq!(t_in("de", "no.such.key"), "no.such.key");
    }

    #[test]
    fn format_fills_named_placeholders() {
        assert_eq!(
            format_in("en", "recorder.duplicate", &[("action", "Center")]),
            "Already used by Center."
        );
        assert_eq!(
            format_in("de", "about.version", &[("version", "1.2.3")]),
            "Version 1.2.3"
        );
    }

    #[test]
    fn pick_supported_uses_the_first_known_language() {
        assert_eq!(pick_supported(&["pt-BR", "en-US"]), "pt");
        assert_eq!(pick_supported(&["sv-SE", "ja-JP", "de-DE"]), "ja");
        assert_eq!(pick_supported(&["sv-SE", "nl-NL"]), "en");
        assert_eq!(pick_supported(&["ZH-hans-CN"]), "zh");
        assert_eq!(pick_supported::<&str>(&[]), "en");
        assert_eq!(pick_supported(&["e"]), "en");
    }

    #[test]
    fn set_rejects_unknown_codes() {
        assert!(!set("xx"));
        assert!(is_supported("uk"));
        assert!(!is_supported("EN"));
    }
}
