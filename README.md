# Wectangle

A Rectangle-style keyboard window manager for Windows, written in Rust. It
moves and resizes the focused window with global keyboard shortcuts: no
dragging, no snap zones. Repeating a shortcut cycles through a configurable
set of sizes.

## Features

- Global shortcuts to snap the focused window to a side, a corner, the top,
  the bottom, or the center of its monitor.
- Repeating a shortcut steps through a size cycle. Pick any of 1/2, 2/3, 3/4,
  1/4 and 1/3 of the work area in Settings (1/2, 2/3 and 3/4 by default).
- Maximize and restore, and moving a window to the next or previous monitor.
- Shortcuts that Windows or another app already owns, such as
  Alt+Win+Left/Right, are taken over instead of refused (see Shortcut
  takeover).
- A tray icon with a Windows 11 style Settings window (Slint, Fluent style,
  following the system light or dark theme) for changing any shortcut, the
  size cycle, and whether Wectangle launches at startup. Changes apply as you
  make them. Settings also names the app that a taken-over shortcut
  overrides, for example "Overrides PowerToys Peek".
- Twelve languages (German, English, Spanish, French, Italian, Polish,
  Portuguese, Russian, Ukrainian, Chinese, Japanese and Korean), chosen in
  Settings under General. The first run follows the Windows display language.
  Settings, the tray menu and notifications switch as soon as you pick one.
- Update checks against GitHub Releases, on demand or once a day (see
  Updates).
- No admin rights required, unless the window you want to move is itself
  running as administrator (see Limitations).

## Default shortcuts

| Action | Default |
| --- | --- |
| Left (full height) | Ctrl+Alt+Left |
| Right (full height) | Ctrl+Alt+Right |
| Top (full width) | Ctrl+Alt+Up |
| Bottom (full width) | Ctrl+Alt+Down |
| Top-left | Ctrl+Alt+F |
| Top-right | Ctrl+Alt+G |
| Bottom-left | Ctrl+Alt+V |
| Bottom-right | Ctrl+Alt+B |
| Maximize / restore | Ctrl+Alt+Enter |
| Center | Ctrl+Alt+Home |
| Next display | Ctrl+Alt+Win+Right |
| Previous display | Ctrl+Alt+Win+Left |

Every shortcut can be changed from the tray icon's Settings window.

## Build and run

Requires a stable Rust toolchain (see `rust-toolchain.toml`; rustup installs
it automatically), the MSVC "Desktop development with C++" build tools, and
a Windows 10 SDK, since the build links against Win32 and embeds an icon and
manifest.

```
cargo build --release
target\release\wectangle.exe
```

On Windows, double-click `run.bat` to build whatever changed and (re)start
Wectangle, or `build.bat` to only build it.

Running the executable a second time brings up the Settings window of the
already running instance instead of starting a second copy.

### Installing it for everyday use

Copy `target\release\wectangle.exe` somewhere permanent, for example
`%LOCALAPPDATA%\Programs\Wectangle`, run it from there, and turn on "Launch
at startup" from the tray menu or the Settings window.

### Releasing

Bump `version` in `Cargo.toml`, then push a tag named `v` plus that version
(for example `v0.2.0`). The Release workflow checks the tag against
`Cargo.toml`, builds `wectangle.exe` and publishes it as a GitHub release.

### Developer overrides

- `WECTANGLE_THEME=light` or `dark` forces the Settings theme instead of
  following Windows.
- `WECTANGLE_UPDATE_URL` replaces the GitHub releases API URL for the update
  check, so a local server can stand in for GitHub. Plain `http://` is
  accepted only through this variable.

### Enabling the pre-commit hook

This repository ships a hook that checks formatting and rejects em or en
dashes in `src/*.rs`. After cloning, enable it once with:

```
git config core.hooksPath .githooks
```

## Configuration

Settings are stored at `%APPDATA%\Wectangle\config.json`. It holds the size
cycle, the shortcut assigned to each action, `language` (a two letter code),
`check_updates` (default `true`) and `last_update_check` (a Unix time in
seconds, maintained by Wectangle).
Edit it through the Settings window; a hand-edited file that fails to parse
falls back to defaults with a notification, and is left untouched until you
change something in Settings.

## Shortcut takeover

Wectangle first asks Windows to register each shortcut. When another app, or
Windows itself, already owns one, Wectangle takes it over with one global
low-level keyboard hook, installed only while at least one shortcut needs it.
While the hook is on, the taken-over chord triggers Wectangle's action and is
hidden from the owner and from the focused app. Settings shows the status as
"Overrides <app>".

- Win+L and Ctrl+Alt+Del are handled by Windows below the hook, so they
  cannot be taken over. Settings shows "Reserved by Windows" for them.
- The owner name comes from the other app's own hotkey settings (PowerToys,
  Twinkle Tray, the NVIDIA overlay, the Claude desktop Quick Entry shortcut),
  from a table of built-in Windows chords, and, marked "probably", from
  Windows Magnifier and the default Claude shortcut while those apps run.
  Only hotkey fields are read; tokens and other settings in those files are
  ignored.
- The hook runs on Wectangle's UI thread. If that thread stalls, Windows can
  stop calling the hook, and taken-over shortcuts stop working until
  Wectangle restarts.

## Updates

Wectangle asks `api.github.com` for the latest release of this repository 30
seconds after it starts, if 24 hours have passed since the last check, and
then every 24 hours. When a newer version exists, a tray notification says so
and clicking it opens the download in your browser. Wectangle never installs
anything itself.

Turn the automatic check off with "Check for updates automatically" in
Settings. The tray menu's "Check for updates..." and Settings' "Check now"
check on demand.

## Limitations

- **Admin windows.** Windows running as administrator cannot be moved by a
  non-elevated Wectangle, because of UIPI. Wectangle detects this up front
  and shows a one-time notification instead of failing silently.
- **AltGr note.** On a US-International keyboard layout, Ctrl+Alt+<key> is
  the same key chord as AltGr+<key>. Assigning a shortcut that way can block
  typing that key's AltGr character anywhere else. The Settings window flags
  any shortcut that does this.
- **Default keys also used elsewhere.** A few defaults overlap with shortcuts
  some other programs already use:
  - **Magnifier**, while it is running: Ctrl+Alt+Arrows, F, Enter.
  - **VS Code**: Ctrl+Alt+Up/Down add cursors.
  - **JetBrains IDEs**: Ctrl+Alt+Left/Right navigate back and forward.

  Change any of these from the Settings window if they conflict with how you
  work.

## Credits

- Inspired by [Rectangle](https://rectangleapp.com) by Ryan Hanson, the macOS window manager.
- The Settings window is built with [Slint](https://slint.dev).

[![#MadeWithSlint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)
