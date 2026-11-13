//! Xfce's keyboard shortcuts, as `xfconf-query -c xfce4-keyboard-shortcuts
//! -l -v` lists them. On X11 a key another app has grabbed can't be grabbed
//! again, so Wrecktangle takes over a chord Xfce uses by removing Xfce's
//! binding while it runs and putting it back afterwards. This is the pure
//! part: parsing, owner names and the record of what was removed. The
//! `xfconf-query` calls on the `xfce4-keyboard-shortcuts` channel are in
//! `linux/xfconf.rs`.

use serde::{Deserialize, Serialize};

use crate::lang;
use crate::owners::OwnerName;
use crate::shortcut::Shortcut;

// Xfce reads only the `custom` set; `default` is what "Reset to defaults"
// copies back.
const WM_PREFIX: &str = "/xfwm4/custom/";
const COMMAND_PREFIX: &str = "/commands/custom/";

/// One Xfce shortcut: its xfconf property, such as
/// `/xfwm4/custom/<Primary><Alt>Left`, and its string value, the window
/// manager action (`left_workspace_key`) or the command (`thunar`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub property: String,
    pub value: String,
}

impl Binding {
    fn accelerator(&self) -> Option<&str> {
        let rest = self
            .property
            .strip_prefix(WM_PREFIX)
            .or_else(|| self.property.strip_prefix(COMMAND_PREFIX))?;
        // `override` is a flag next to the shortcuts, not one of them.
        (!rest.is_empty() && !rest.contains('/') && rest != "override").then_some(rest)
    }

    pub fn shortcut(&self) -> Option<Shortcut> {
        parse_accelerator(self.accelerator()?)
    }

    pub fn owner(&self) -> OwnerName {
        if self.property.starts_with(WM_PREFIX) {
            OwnerName::System(wm_action_key(&self.value))
        } else {
            OwnerName::Product(lang::format(
                "owner.xfce_command",
                &[("command", &program(&self.value))],
            ))
        }
    }
}

/// The shortcuts in `xfconf-query -l -v` output: one property per line,
/// then spaces, then its value. Sub-properties such as
/// `/commands/custom/<Super>r/startup-notify` and the `override` flag are
/// left out.
pub fn parse_list(output: &str) -> Vec<Binding> {
    output
        .lines()
        .filter_map(|line| {
            let (property, value) = line.split_once(char::is_whitespace)?;
            let binding = Binding {
                property: property.to_string(),
                value: value.trim().to_string(),
            };
            binding.accelerator().is_some().then_some(binding)
        })
        .collect()
}

/// Parses a GTK accelerator such as `<Primary><Alt>Left`. None for a
/// modifier or key a Wrecktangle shortcut can't use, such as `<Hyper>` or
/// `XF86AudioMute`.
pub fn parse_accelerator(accelerator: &str) -> Option<Shortcut> {
    let mut sc = Shortcut {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        vk: 0,
    };
    let mut rest = accelerator.trim();
    while let Some(tail) = rest.strip_prefix('<') {
        let (modifier, tail) = tail.split_once('>')?;
        match modifier.to_ascii_lowercase().as_str() {
            "primary" | "control" | "ctrl" => sc.ctrl = true,
            "alt" | "mod1" => sc.alt = true,
            "shift" => sc.shift = true,
            "super" | "mod4" => sc.win = true,
            _ => return None,
        }
        rest = tail;
    }
    sc.vk = key_from_name(rest)?;
    Some(sc)
}

fn key_from_name(name: &str) -> Option<u16> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(c.to_ascii_uppercase() as u16);
    }
    let lower = name.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x6F + n);
    }
    if let Some(n) = lower
        .strip_prefix("kp_")
        .and_then(|n| n.parse::<u16>().ok())
        && n <= 9
    {
        return Some(0x60 + n);
    }
    let vk = match lower.as_str() {
        // The keypad with NumLock off is the same key as its digit.
        "kp_insert" => 0x60,
        "kp_end" => 0x61,
        "kp_down" => 0x62,
        "kp_next" | "kp_page_down" => 0x63,
        "kp_left" => 0x64,
        "kp_begin" => 0x65,
        "kp_right" => 0x66,
        "kp_home" => 0x67,
        "kp_up" => 0x68,
        "kp_prior" | "kp_page_up" => 0x69,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "return" => 0x0D,
        "space" => 0x20,
        "tab" | "iso_left_tab" => 0x09,
        "backspace" => 0x08,
        "delete" => 0x2E,
        "insert" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "prior" | "page_up" => 0x21,
        "next" | "page_down" => 0x22,
        "semicolon" => 0xBA,
        "equal" => 0xBB,
        "comma" => 0xBC,
        "minus" => 0xBD,
        "period" => 0xBE,
        "slash" => 0xBF,
        "grave" => 0xC0,
        "bracketleft" => 0xDB,
        "backslash" => 0xDC,
        "bracketright" => 0xDD,
        "apostrophe" => 0xDE,
        _ => return None,
    };
    Some(vk)
}

fn wm_action_key(action: &str) -> &'static str {
    let workspace_number = action
        .strip_prefix("workspace_")
        .and_then(|rest| rest.strip_suffix("_key"))
        .is_some_and(|n| n.parse::<u32>().is_ok());
    match action {
        "left_workspace_key"
        | "right_workspace_key"
        | "up_workspace_key"
        | "down_workspace_key"
        | "next_workspace_key"
        | "prev_workspace_key" => "owner.xfce_workspaces",
        _ if workspace_number => "owner.xfce_workspaces",
        _ if action.starts_with("move_window_") && action.contains("workspace") => {
            "owner.xfce_move_workspace"
        }
        "show_desktop_key" => "owner.xfce_show_desktop",
        _ if action.starts_with("tile_") => "owner.xfce_tiling",
        "cycle_windows_key"
        | "cycle_reverse_windows_key"
        | "switch_window_key"
        | "switch_application_key" => "owner.xfce_window_switcher",
        _ => "owner.xfce_window_manager",
    }
}

/// The program a command shortcut runs, without its path or arguments.
fn program(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or(command);
    first.rsplit('/').next().unwrap_or(first).to_string()
}

/// The bindings that hold one of `wanted`'s chords.
pub fn clashing<'a>(wanted: &[Shortcut], bindings: &'a [Binding]) -> Vec<&'a Binding> {
    bindings
        .iter()
        .filter(|binding| binding.shortcut().is_some_and(|sc| wanted.contains(&sc)))
        .collect()
}

/// The bindings Wrecktangle removed, kept on disk so a crash or a killed
/// process can't lose them: the next start, or the uninstall script, puts
/// them back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub removed: Vec<Binding>,
}

impl Record {
    pub fn from_json(text: &str) -> Record {
        serde_json::from_str(text).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a record always serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::parse;

    const LIST: &str = "\
/commands/custom/<Primary><Alt>Delete      xflock4
/commands/custom/<Primary><Alt>f           thunar
/commands/custom/<Super>p                  xfce4-display-settings --minimal
/commands/custom/<Super>r                  xfce4-appfinder -c
/commands/custom/<Super>r/startup-notify   true
/commands/custom/override                  true
/commands/default/<Primary><Alt>t          exo-open --launch TerminalEmulator
/xfwm4/custom/<Primary><Alt>Left           left_workspace_key
/xfwm4/custom/<Primary><Alt>End            move_window_next_workspace_key
/xfwm4/custom/<Super>KP_Down               tile_down_key
/xfwm4/custom/<Alt>Tab                     cycle_windows_key
/xfwm4/custom/<Primary><Alt>KP_3           move_window_workspace_3_key
/xfwm4/custom/<Primary>F3                  workspace_3_key
/xfwm4/custom/<Alt>F10                     maximize_window_key
/xfwm4/custom/XF86Display                  some_key
/xfwm4/custom/override                     true
";

    fn bindings() -> Vec<Binding> {
        parse_list(LIST)
    }

    fn find(property: &str) -> Binding {
        bindings()
            .into_iter()
            .find(|b| b.property == property)
            .unwrap_or_else(|| panic!("{property} is listed"))
    }

    #[test]
    fn lists_only_custom_shortcuts() {
        let properties: Vec<String> = bindings().into_iter().map(|b| b.property).collect();
        assert!(properties.contains(&"/commands/custom/<Super>r".to_string()));
        assert!(properties.contains(&"/xfwm4/custom/XF86Display".to_string()));
        assert!(!properties.iter().any(|p| p.ends_with("/startup-notify")));
        assert!(!properties.iter().any(|p| p.ends_with("/override")));
        assert!(!properties.iter().any(|p| p.contains("/default/")));
        assert_eq!(properties.len(), 12);
    }

    #[test]
    fn keeps_the_whole_command() {
        assert_eq!(
            find("/commands/custom/<Super>p").value,
            "xfce4-display-settings --minimal"
        );
    }

    #[test]
    fn accelerators_parse_into_shortcuts() {
        assert_eq!(
            parse_accelerator("<Primary><Alt>Left"),
            parse("Ctrl+Alt+Left").ok()
        );
        assert_eq!(
            parse_accelerator("<Control><Shift><Super>f"),
            parse("Ctrl+Shift+Win+F").ok()
        );
        assert_eq!(parse_accelerator("<Mod4>Return"), parse("Win+Enter").ok());
        assert_eq!(parse_accelerator("<Alt>F10"), parse("Alt+F10").ok());
        assert_eq!(
            parse_accelerator("<Primary><Alt>Page_Down"),
            parse("Ctrl+Alt+PageDown").ok()
        );
    }

    #[test]
    fn keypad_names_with_numlock_off_are_the_digit_keys() {
        assert_eq!(
            parse_accelerator("<Super>KP_Down"),
            parse("Win+Numpad2").ok()
        );
        assert_eq!(
            parse_accelerator("<Super>KP_Home"),
            parse("Win+Numpad7").ok()
        );
        assert_eq!(
            parse_accelerator("<Primary><Alt>KP_3"),
            parse("Ctrl+Alt+Numpad3").ok()
        );
    }

    #[test]
    fn unusable_accelerators_are_rejected() {
        assert_eq!(parse_accelerator("XF86Display"), None);
        assert_eq!(parse_accelerator("<Hyper>a"), None);
        assert_eq!(parse_accelerator("<Primary>"), None);
        assert_eq!(parse_accelerator("<Primary"), None);
    }

    #[test]
    fn window_manager_actions_get_owner_names() {
        let key = |property: &str| match find(property).owner() {
            OwnerName::System(key) => key,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            key("/xfwm4/custom/<Primary><Alt>Left"),
            "owner.xfce_workspaces"
        );
        assert_eq!(key("/xfwm4/custom/<Primary>F3"), "owner.xfce_workspaces");
        assert_eq!(
            key("/xfwm4/custom/<Primary><Alt>End"),
            "owner.xfce_move_workspace"
        );
        assert_eq!(
            key("/xfwm4/custom/<Primary><Alt>KP_3"),
            "owner.xfce_move_workspace"
        );
        assert_eq!(key("/xfwm4/custom/<Super>KP_Down"), "owner.xfce_tiling");
        assert_eq!(key("/xfwm4/custom/<Alt>Tab"), "owner.xfce_window_switcher");
        assert_eq!(key("/xfwm4/custom/<Alt>F10"), "owner.xfce_window_manager");
    }

    #[test]
    fn command_owners_name_the_program() {
        assert_eq!(
            find("/commands/custom/<Primary><Alt>f").owner(),
            OwnerName::Product(lang::format_in(
                "en",
                "owner.xfce_command",
                &[("command", "thunar")]
            ))
        );
        assert_eq!(program("/usr/bin/xfce4-appfinder -c"), "xfce4-appfinder");
    }

    #[test]
    fn clashing_finds_the_bindings_of_wanted_chords() {
        let all = bindings();
        let wanted = [
            parse("Ctrl+Alt+Left").unwrap(),
            parse("Ctrl+Alt+F").unwrap(),
            parse("Ctrl+Alt+G").unwrap(),
        ];
        let clashing: Vec<&str> = clashing(&wanted, &all)
            .into_iter()
            .map(|b| b.property.as_str())
            .collect();
        assert_eq!(
            clashing,
            [
                "/commands/custom/<Primary><Alt>f",
                "/xfwm4/custom/<Primary><Alt>Left"
            ]
        );
    }

    #[test]
    fn records_round_trip_and_bad_files_read_as_empty() {
        let record = Record {
            removed: vec![find("/xfwm4/custom/<Primary><Alt>Left")],
        };
        assert_eq!(Record::from_json(&record.to_json()), record);
        assert_eq!(Record::from_json("not json"), Record::default());
    }
}
