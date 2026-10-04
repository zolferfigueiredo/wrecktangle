use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::Action;
use crate::shortcut::{self, Shortcut};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseSizeError;

pub fn parse_size(input: &str) -> Result<f64, ParseSizeError> {
    let input = input.trim();
    let fraction = if let Some(percent) = input.strip_suffix('%') {
        let value: f64 = percent.trim().parse().map_err(|_| ParseSizeError)?;
        value / 100.0
    } else if let Some((num, den)) = input.split_once('/') {
        let n: f64 = num.trim().parse().map_err(|_| ParseSizeError)?;
        let d: f64 = den.trim().parse().map_err(|_| ParseSizeError)?;
        if d == 0.0 {
            return Err(ParseSizeError);
        }
        n / d
    } else {
        input.parse().map_err(|_| ParseSizeError)?
    };

    if fraction > 0.0 && fraction <= 1.0 {
        Ok(fraction)
    } else {
        Err(ParseSizeError)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Missing,
    Loaded,
    Invalid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shortcuts {
    pub left: Option<Shortcut>,
    pub right: Option<Shortcut>,
    pub top: Option<Shortcut>,
    pub bottom: Option<Shortcut>,
    pub top_left: Option<Shortcut>,
    pub top_right: Option<Shortcut>,
    pub bottom_left: Option<Shortcut>,
    pub bottom_right: Option<Shortcut>,
    pub maximize: Option<Shortcut>,
    pub center: Option<Shortcut>,
    pub next_display: Option<Shortcut>,
    pub previous_display: Option<Shortcut>,
}

impl Shortcuts {
    pub fn get(&self, action: Action) -> Option<Shortcut> {
        match action {
            Action::Left => self.left,
            Action::Right => self.right,
            Action::Top => self.top,
            Action::Bottom => self.bottom,
            Action::TopLeft => self.top_left,
            Action::TopRight => self.top_right,
            Action::BottomLeft => self.bottom_left,
            Action::BottomRight => self.bottom_right,
            Action::Maximize => self.maximize,
            Action::Center => self.center,
            Action::NextDisplay => self.next_display,
            Action::PreviousDisplay => self.previous_display,
        }
    }

    pub fn entries(&self) -> [(Action, Option<Shortcut>); 12] {
        Action::ALL.map(|action| (action, self.get(action)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub sizes: Vec<String>,
    pub shortcuts: Shortcuts,
    pub check_updates: bool,
    pub last_update_check: u64,
}

impl Config {
    pub fn defaults() -> Config {
        resolve(ConfigFile::default()).expect("default config must resolve")
    }

    pub fn size_fractions(&self) -> Vec<f64> {
        self.sizes
            .iter()
            .filter_map(|s| parse_size(s).ok())
            .collect()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    sizes: Option<Vec<String>>,
    #[serde(default)]
    shortcuts: Option<ShortcutsFile>,
    #[serde(default)]
    check_updates: Option<bool>,
    #[serde(default)]
    last_update_check: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ShortcutsFile {
    #[serde(default)]
    left: Option<String>,
    #[serde(default)]
    right: Option<String>,
    #[serde(default)]
    top: Option<String>,
    #[serde(default)]
    bottom: Option<String>,
    #[serde(default)]
    top_left: Option<String>,
    #[serde(default)]
    top_right: Option<String>,
    #[serde(default)]
    bottom_left: Option<String>,
    #[serde(default)]
    bottom_right: Option<String>,
    #[serde(default)]
    maximize: Option<String>,
    #[serde(default)]
    center: Option<String>,
    #[serde(default)]
    next_display: Option<String>,
    #[serde(default)]
    previous_display: Option<String>,
}

fn default_size_strings() -> Vec<String> {
    ["1/2", "2/3", "1/3", "2/7"]
        .into_iter()
        .map(String::from)
        .collect()
}

// A missing key defaults, an explicit "" is unbound, anything else must
// parse; the outer Option is None when the file should count as invalid.
fn resolve_shortcut(raw: Option<String>, default_str: &str) -> Option<Option<Shortcut>> {
    match raw {
        None => Some(Some(
            shortcut::parse(default_str).expect("built in default shortcut must parse"),
        )),
        Some(s) if s.is_empty() => Some(None),
        Some(s) => shortcut::parse(&s).ok().map(Some),
    }
}

fn resolve(file: ConfigFile) -> Option<Config> {
    let sizes = match file.sizes {
        Some(list) => {
            if list.is_empty() || list.len() > 8 {
                return None;
            }
            for s in &list {
                parse_size(s).ok()?;
            }
            list
        }
        None => default_size_strings(),
    };

    let sf = file.shortcuts.unwrap_or_default();
    let shortcuts = Shortcuts {
        left: resolve_shortcut(sf.left, "Ctrl+Alt+Left")?,
        right: resolve_shortcut(sf.right, "Ctrl+Alt+Right")?,
        top: resolve_shortcut(sf.top, "Ctrl+Alt+Up")?,
        bottom: resolve_shortcut(sf.bottom, "Ctrl+Alt+Down")?,
        top_left: resolve_shortcut(sf.top_left, "Ctrl+Alt+F")?,
        top_right: resolve_shortcut(sf.top_right, "Ctrl+Alt+G")?,
        bottom_left: resolve_shortcut(sf.bottom_left, "Ctrl+Alt+V")?,
        bottom_right: resolve_shortcut(sf.bottom_right, "Ctrl+Alt+B")?,
        maximize: resolve_shortcut(sf.maximize, "Ctrl+Alt+Enter")?,
        center: resolve_shortcut(sf.center, "Ctrl+Alt+Home")?,
        next_display: resolve_shortcut(sf.next_display, "Ctrl+Alt+Win+Right")?,
        previous_display: resolve_shortcut(sf.previous_display, "Ctrl+Alt+Win+Left")?,
    };

    Some(Config {
        sizes,
        shortcuts,
        check_updates: file.check_updates.unwrap_or(true),
        last_update_check: file.last_update_check.unwrap_or(0),
    })
}

// A field absent from otherwise-valid JSON falls back to its default; a
// parse failure anywhere makes the whole file Invalid, so the caller can
// notify and must not overwrite it until the user presses Save.
pub fn load_from_str(text: &str) -> (Config, Source) {
    match serde_json::from_str::<ConfigFile>(text) {
        Ok(file) => match resolve(file) {
            Some(config) => (config, Source::Loaded),
            None => (Config::defaults(), Source::Invalid),
        },
        Err(_) => (Config::defaults(), Source::Invalid),
    }
}

fn format_opt(shortcut: &Option<Shortcut>) -> String {
    match shortcut {
        Some(s) => shortcut::format(s),
        None => String::new(),
    }
}

pub fn to_json_string(config: &Config) -> String {
    let file = ConfigFile {
        sizes: Some(config.sizes.clone()),
        shortcuts: Some(ShortcutsFile {
            left: Some(format_opt(&config.shortcuts.left)),
            right: Some(format_opt(&config.shortcuts.right)),
            top: Some(format_opt(&config.shortcuts.top)),
            bottom: Some(format_opt(&config.shortcuts.bottom)),
            top_left: Some(format_opt(&config.shortcuts.top_left)),
            top_right: Some(format_opt(&config.shortcuts.top_right)),
            bottom_left: Some(format_opt(&config.shortcuts.bottom_left)),
            bottom_right: Some(format_opt(&config.shortcuts.bottom_right)),
            maximize: Some(format_opt(&config.shortcuts.maximize)),
            center: Some(format_opt(&config.shortcuts.center)),
            next_display: Some(format_opt(&config.shortcuts.next_display)),
            previous_display: Some(format_opt(&config.shortcuts.previous_display)),
        }),
        check_updates: Some(config.check_updates),
        last_update_check: Some(config.last_update_check),
    };
    serde_json::to_string_pretty(&file).expect("config always serializes")
}

pub fn config_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA must be set on Windows");
    Path::new(&appdata).join("Wectangle").join("config.json")
}

pub fn load(path: &Path) -> (Config, Source) {
    match fs::read_to_string(path) {
        Ok(text) => load_from_str(&text),
        Err(_) => (Config::defaults(), Source::Missing),
    }
}

pub fn save(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, to_json_string(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_fraction() {
        let f = parse_size("2/7").unwrap();
        assert!((f - 2.0 / 7.0).abs() < 1e-9);
    }

    #[test]
    fn parse_size_percent() {
        let f = parse_size("40%").unwrap();
        assert!((f - 0.4).abs() < 1e-9);
    }

    #[test]
    fn parse_size_rejects_invalid_input() {
        assert!(parse_size("abc").is_err());
        assert!(parse_size("").is_err());
        assert!(parse_size("0").is_err());
        assert!(parse_size("1/0").is_err());
        assert!(parse_size("150%").is_err());
        assert!(parse_size("-10%").is_err());
    }

    #[test]
    fn defaults_match_the_documented_table() {
        let config = Config::defaults();
        assert_eq!(config.sizes, vec!["1/2", "2/3", "1/3", "2/7"]);
        assert_eq!(config.shortcuts.left, shortcut::parse("Ctrl+Alt+Left").ok());
        assert_eq!(
            config.shortcuts.right,
            shortcut::parse("Ctrl+Alt+Right").ok()
        );
        assert_eq!(config.shortcuts.top, shortcut::parse("Ctrl+Alt+Up").ok());
        assert_eq!(
            config.shortcuts.bottom,
            shortcut::parse("Ctrl+Alt+Down").ok()
        );
        assert_eq!(
            config.shortcuts.top_left,
            shortcut::parse("Ctrl+Alt+F").ok()
        );
        assert_eq!(
            config.shortcuts.top_right,
            shortcut::parse("Ctrl+Alt+G").ok()
        );
        assert_eq!(
            config.shortcuts.bottom_left,
            shortcut::parse("Ctrl+Alt+V").ok()
        );
        assert_eq!(
            config.shortcuts.bottom_right,
            shortcut::parse("Ctrl+Alt+B").ok()
        );
        assert_eq!(
            config.shortcuts.maximize,
            shortcut::parse("Ctrl+Alt+Enter").ok()
        );
        assert_eq!(
            config.shortcuts.center,
            shortcut::parse("Ctrl+Alt+Home").ok()
        );
        assert_eq!(
            config.shortcuts.next_display,
            shortcut::parse("Ctrl+Alt+Win+Right").ok()
        );
        assert_eq!(
            config.shortcuts.previous_display,
            shortcut::parse("Ctrl+Alt+Win+Left").ok()
        );
    }

    #[test]
    fn update_fields_default_to_checking_with_no_previous_check() {
        let config = Config::defaults();
        assert!(config.check_updates);
        assert_eq!(config.last_update_check, 0);
    }

    #[test]
    fn config_without_update_fields_still_loads() {
        let (config, source) = load_from_str(r#"{"sizes": ["1/2"], "shortcuts": {"left": ""}}"#);
        assert_eq!(source, Source::Loaded);
        assert!(config.check_updates);
        assert_eq!(config.last_update_check, 0);
        assert_eq!(config.shortcuts.left, None);
    }

    #[test]
    fn update_fields_are_read_and_round_trip() {
        let (config, source) =
            load_from_str(r#"{"check_updates": false, "last_update_check": 1760000000}"#);
        assert_eq!(source, Source::Loaded);
        assert!(!config.check_updates);
        assert_eq!(config.last_update_check, 1_760_000_000);

        let (reloaded, source) = load_from_str(&to_json_string(&config));
        assert_eq!(source, Source::Loaded);
        assert_eq!(reloaded, config);
    }

    #[test]
    fn wrongly_typed_update_field_is_invalid() {
        let (config, source) = load_from_str(r#"{"check_updates": "yes"}"#);
        assert_eq!(source, Source::Invalid);
        assert_eq!(config, Config::defaults());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults_individually() {
        let (config, source) = load_from_str(r#"{"sizes": ["1/4"]}"#);
        assert_eq!(source, Source::Loaded);
        assert_eq!(config.sizes, vec!["1/4"]);
        assert_eq!(config.shortcuts, Config::defaults().shortcuts);
    }

    #[test]
    fn empty_object_yields_defaults() {
        let (config, source) = load_from_str("{}");
        assert_eq!(source, Source::Loaded);
        assert_eq!(config, Config::defaults());
    }

    #[test]
    fn explicit_empty_string_means_unbound() {
        let (config, source) = load_from_str(r#"{"shortcuts": {"center": ""}}"#);
        assert_eq!(source, Source::Loaded);
        assert_eq!(config.shortcuts.center, None);
        assert_eq!(config.shortcuts.left, Config::defaults().shortcuts.left);
    }

    #[test]
    fn malformed_json_is_invalid_and_falls_back_to_defaults() {
        let (config, source) = load_from_str("{not json");
        assert_eq!(source, Source::Invalid);
        assert_eq!(config, Config::defaults());
    }

    #[test]
    fn unparseable_value_is_invalid_and_falls_back_to_defaults() {
        let (config, source) = load_from_str(r#"{"sizes": ["not a size"]}"#);
        assert_eq!(source, Source::Invalid);
        assert_eq!(config, Config::defaults());

        let (config, source) = load_from_str(r#"{"shortcuts": {"left": "NotAKey"}}"#);
        assert_eq!(source, Source::Invalid);
        assert_eq!(config, Config::defaults());
    }

    #[test]
    fn too_many_sizes_is_invalid() {
        let sizes: Vec<String> = (1..=9).map(|i| format!("1/{}", i + 10)).collect();
        let json = serde_json::json!({ "sizes": sizes }).to_string();
        let (config, source) = load_from_str(&json);
        assert_eq!(source, Source::Invalid);
        assert_eq!(config, Config::defaults());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("wectangle-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");

        let config = Config::defaults();
        save(&path, &config).unwrap();
        let (loaded, source) = load(&path);
        assert_eq!(source, Source::Loaded);
        assert_eq!(loaded, config);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_reports_missing_file() {
        let dir =
            std::env::temp_dir().join(format!("wectangle-test-missing-{}", std::process::id()));
        let path = dir.join("config.json");
        let (config, source) = load(&path);
        assert_eq!(source, Source::Missing);
        assert_eq!(config, Config::defaults());
    }
}
