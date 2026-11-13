//! The Settings controller on Linux: keeps the window's model in sync with
//! the config, runs what the window asks for, and records shortcuts with a
//! keyboard grab. The counterpart of `src/settings.rs`; the window itself is
//! in `window`.

mod gfx;
mod window;

use std::cell::Cell;

use x11rb::protocol::Event;

use super::{app, tray, window_ops};
use crate::config::Config;
use crate::layout::Action;
use crate::shortcut::{Shortcut, keysym};
use crate::theme::Theme;
use crate::ui::model;
use crate::ui::session::Command;
use crate::ui::view::Row;
use crate::{lang, startup, update};

const KEY_ESCAPE: u32 = 0xFF1B;

thread_local! {
    static RECORDING: Cell<Option<usize>> = const { Cell::new(None) };
}

pub fn open() {
    if !window::exists() {
        if !window::create() {
            return;
        }
        refresh_all();
    }
    window::show();
}

/// Handles an event for the Settings window; false when it is not one.
pub fn handle_event(event: &Event) -> bool {
    window::handle_event(event)
}

pub fn paint_if_dirty() {
    window::paint_if_dirty();
}

fn run(command: Command) {
    match command {
        Command::Page(page) => {
            if window::set_page(page) {
                cancel_recording();
            }
        }
        Command::Record(index) => start_recording(index),
        Command::Clear(index) => {
            let mut config = app::current_config();
            config.shortcuts.set(Action::ALL[index], None);
            commit(config);
        }
        Command::RestoreDefaults => {
            let mut config = app::current_config();
            config.shortcuts = Config::defaults().shortcuts;
            commit(config);
        }
        Command::LanguageMenu => {
            // The recorder's keyboard grab would keep the list's keys.
            cancel_recording();
            window::open_language_popup(lang::current_index());
        }
        Command::Language(index) => set_language(index),
        Command::ThemeMenu => {
            cancel_recording();
            window::open_theme_popup(app::current_config().theme);
        }
        Command::SetTheme(theme) => set_theme(theme),
        Command::SetColors(on) => {
            app::update_config(|config| config.wrecktangle_colors = on);
            let on = app::current_config().wrecktangle_colors;
            window::update(|m| m.wrecktangle_colors = on);
        }
        Command::ToggleSize(index) => on_size_toggled(index),
        Command::SetStartup(enable) => set_startup(enable),
        Command::SetAutoUpdate(enable) => {
            update::set_auto(enable);
            let enabled = update::auto_enabled();
            window::update(|m| m.auto_update = enabled);
        }
        // Not shown on Linux.
        Command::SetAutoInstall(_) | Command::SetBlockSnap(_) => {}
        Command::CheckNow => {
            update::check_now();
            on_update_state_changed();
        }
        Command::Update => update::install(),
        Command::OpenUrl(url) => update::open_url(url),
    }
}

/// The window was closed; it is destroyed with its fonts, and the next open
/// builds it again.
fn on_close() {
    cancel_recording();
    window::destroy();
}

fn refresh_all() {
    show_sizes();
    let startup = startup::is_enabled();
    let auto_update = update::auto_enabled();
    let language = lang::current_index();
    let config = app::current_config();
    let displays = window_ops::connected_display_count();
    window::update(|m| {
        m.theme = config.theme;
        m.wrecktangle_colors = config.wrecktangle_colors;
        m.displays = displays;
        m.startup = startup;
        m.auto_update = auto_update;
        m.language = language;
        m.hide_auto_install = true;
        m.hide_block_snap = true;
    });
    show_update_state();
    refresh_rows();
}

/// Screens were connected or removed; Display mode's row follows.
pub fn show_displays() {
    if window::exists() {
        let displays = window_ops::connected_display_count();
        window::update(|m| m.displays = displays);
    }
}

fn set_language(index: usize) {
    let Some(language) = lang::LANGUAGES.get(index) else {
        return;
    };
    if lang::current_index() == index {
        return;
    }
    lang::set(language.code);
    let mut config = app::current_config();
    config.language = language.code.to_string();
    commit(config);
    window::update(|m| m.language = index);
    window::language_changed();
    show_update_state();
    tray::refresh();
}

fn set_theme(theme: Theme) {
    app::update_config(|config| config.theme = theme);
    let theme = app::current_config().theme;
    window::update(|m| m.theme = theme);
    window::retheme();
    tray::set_theme(theme);
}

/// The desktop's theme or accent changed. Safe from any thread: an open
/// Settings window gets a message and repaints on the main thread.
pub fn desktop_changed() {
    window::post_retheme();
}

fn show_update_state() {
    let state = update::state();
    let installable = update::can_install();
    window::update(|m| model::show_update_state(m, &state, installable));
}

/// Refreshes the update row of an open Settings window.
pub fn on_update_state_changed() {
    if window::exists() {
        show_update_state();
    }
}

fn shortcut_row(action: Action, hint: Option<&str>) -> Row {
    let sc = app::current_config().shortcuts.get(action);
    // X11 tells AltGr apart from Ctrl+Alt, so no shortcut blocks typing.
    model::shortcut_row(action, sc, app::shortcut_note(action), None, hint)
}

fn refresh_rows() {
    let rows: Vec<Row> = Action::ALL
        .iter()
        .map(|action| shortcut_row(*action, None))
        .collect();
    window::update(|m| m.rows = rows);
}

fn show_row(index: usize, hint: Option<&str>) {
    let row = shortcut_row(Action::ALL[index], hint);
    window::update(|m| {
        if let Some(slot) = m.rows.get_mut(index) {
            *slot = row;
        }
    });
}

fn show_sizes() {
    let options = model::size_options(&app::current_config().sizes);
    window::update(|m| m.sizes = options);
}

fn on_size_toggled(index: usize) {
    let mut config = app::current_config();
    let updated = crate::config::toggle_size(&config.sizes, index);
    if updated != config.sizes {
        config.sizes = updated;
        commit(config);
    }
    show_sizes();
}

fn set_startup(enable: bool) {
    if enable != startup::is_enabled() {
        let _ = startup::set_enabled(enable);
    }
    let enabled = startup::is_enabled();
    window::update(|m| m.startup = enabled);
    tray::refresh();
}

// Applies and saves right away. A recording in progress ends first, because
// applying grabs the shortcuts again, which recording had let go.
fn commit(config: Config) {
    finish_recording(false);
    app::apply_new_config(config);
    if window::exists() {
        refresh_rows();
    }
}

fn start_recording(index: usize) {
    if !window::exists() || index >= Action::ALL.len() {
        return;
    }
    let previous = RECORDING.with(|r| r.replace(Some(index)));
    match previous {
        Some(previous) if previous == index => return,
        Some(previous) => show_row(previous, None),
        None => {
            app::suspend_hotkeys();
            window::grab_keyboard();
        }
    }
    show_row(index, Some(""));
}

fn cancel_recording() {
    if finish_recording(true).is_some() && window::exists() {
        refresh_rows();
    }
}

// Ends a recording, if one is running. `resume` grabs the shortcuts again
// that start_recording let go; commit() skips it because applying does that.
fn finish_recording(resume: bool) -> Option<usize> {
    let row = RECORDING.with(|r| r.take())?;
    window::ungrab_keyboard();
    if resume {
        app::resume_hotkeys();
    }
    Some(row)
}

/// A key pressed while the recorder holds the keyboard.
fn on_recorder_key(keycode: u8, state: u16) {
    let Some(index) = RECORDING.with(|r| r.get()) else {
        return;
    };
    let key = app::key_info(keycode, state);
    if key.keysym == KEY_ESCAPE {
        cancel_recording();
        return;
    }
    // Modifier keys on their own, and keys a shortcut can't use, wait for
    // the next key.
    let Some(vk) = keysym::to_vk(key.keysym) else {
        return;
    };
    let candidate = Shortcut {
        ctrl: key.ctrl,
        alt: key.alt,
        shift: key.shift,
        win: key.win,
        vk,
    };
    let mut config = app::current_config();
    match model::check_recorded(index, candidate, &config.shortcuts) {
        Ok(sc) => {
            config.shortcuts.set(Action::ALL[index], Some(sc));
            commit(config);
        }
        Err(hint) => show_row(index, Some(&hint)),
    }
}
