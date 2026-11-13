//! Fills the Settings model from the config and the app's state, and checks
//! a recorded shortcut. Shared by the Windows and Linux controllers.

use super::view::{Model, Row, SizeOption};
use crate::config::{self, Shortcuts};
use crate::layout::Action;
use crate::shortcut::{self, Shortcut};
use crate::{lang, update};

/// One shortcut row. `note` is the app's status for a shortcut it could not
/// simply register, `altgr` the character a Ctrl+Alt shortcut keeps from
/// being typed (Windows only), and `hint` the recorder's prompt or message
/// while this row records.
pub fn shortcut_row(
    action: Action,
    sc: Option<Shortcut>,
    note: Option<String>,
    altgr: Option<char>,
    hint: Option<&str>,
) -> Row {
    let (note, warn) = match (note, sc, altgr) {
        (Some(note), _, _) => (note, false),
        (None, Some(sc), Some(ch)) => (
            lang::format(
                "status.altgr",
                &[("char", &ch.to_string()), ("key", &shortcut::key_name(&sc))],
            ),
            true,
        ),
        _ => (String::new(), false),
    };
    Row {
        label: lang::t(action.key()),
        keys: sc.map(|sc| shortcut::key_caps(&sc)).unwrap_or_default(),
        note,
        warn,
        recording: hint.is_some(),
        hint: hint.unwrap_or_default().to_string(),
    }
}

/// The size cycle's check boxes; the last checked one can't be unchecked.
pub fn size_options(sizes: &[String]) -> Vec<SizeOption> {
    let checks = config::size_checks(sizes);
    let checked_count = checks.iter().filter(|checked| **checked).count();
    config::SIZE_OPTIONS
        .iter()
        .enumerate()
        .map(|(index, label)| SizeOption {
            label: label.to_string(),
            checked: checks[index],
            locked: checks[index] && checked_count == 1,
        })
        .collect()
}

/// The About page's version line and update row. `installable` is whether
/// the update can install itself, rather than open its download.
pub fn show_update_state(model: &mut Model, state: &update::State, installable: bool) {
    model.about_version = lang::format("about.version", &[("version", update::current_version())]);
    model.update_detail = update::status_detail(state);
    model.update_available = matches!(state, update::State::Available { .. });
    model.update_installable = installable;
    model.update_busy = matches!(
        state,
        update::State::Checking
            | update::State::Downloading { .. }
            | update::State::Installing { .. }
    );
}

/// Checks a shortcut recorded for row `index`: the shortcut to save, or the
/// hint that says why it can't be used.
pub fn check_recorded(
    index: usize,
    candidate: Shortcut,
    shortcuts: &Shortcuts,
) -> Result<Shortcut, String> {
    if !(candidate.ctrl || candidate.alt || candidate.win) {
        return Err(lang::format(
            "recorder.needs_modifier",
            &[("super", shortcut::WIN_LABEL)],
        ));
    }
    let duplicate = Action::ALL
        .iter()
        .enumerate()
        .find(|&(i, a)| i != index && shortcuts.get(*a) == Some(candidate))
        .map(|(_, a)| lang::t(a.key()));
    match duplicate {
        Some(label) => Err(lang::format("recorder.duplicate", &[("action", &label)])),
        None => Ok(candidate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::shortcut::parse;

    #[test]
    fn rows_show_the_note_first_then_the_altgr_warning() {
        let sc = parse("Ctrl+Alt+E").ok();
        let row = shortcut_row(
            Action::Left,
            sc,
            Some("Overrides X".into()),
            Some('e'),
            None,
        );
        assert_eq!((row.note.as_str(), row.warn), ("Overrides X", false));
        let row = shortcut_row(Action::Left, sc, None, Some('\u{20AC}'), None);
        assert!(row.warn);
        assert!(row.note.contains('\u{20AC}'));
        let row = shortcut_row(Action::Left, None, None, None, Some(""));
        assert!(row.recording && row.keys.is_empty() && row.note.is_empty());
    }

    #[test]
    fn the_only_checked_size_is_locked() {
        let options = size_options(&["2/3".to_string()]);
        assert_eq!(options.len(), config::SIZE_OPTIONS.len());
        assert!(options.iter().filter(|o| o.checked).all(|o| o.locked));
        let options = size_options(&["1/2".to_string(), "2/3".to_string()]);
        assert!(options.iter().all(|o| !o.locked));
    }

    #[test]
    fn recorded_shortcuts_need_a_modifier_and_must_be_unused() {
        let shortcuts = Config::defaults().shortcuts;
        assert!(check_recorded(0, parse("Shift+Ctrl+K").unwrap(), &shortcuts).is_ok());
        let plain = Shortcut {
            ctrl: false,
            alt: false,
            shift: true,
            win: false,
            vk: 0x4B,
        };
        assert!(check_recorded(0, plain, &shortcuts).is_err());
        // Right's own shortcut, recorded on Left's row.
        let right = shortcuts.get(Action::Right).unwrap();
        assert!(check_recorded(0, right, &shortcuts).is_err());
        assert!(check_recorded(1, right, &shortcuts).is_ok());
    }
}
