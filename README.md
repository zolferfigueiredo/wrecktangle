# Wectangle

A Rectangle-style keyboard window manager for Windows, written in Rust. It
moves and resizes the focused window with global keyboard shortcuts: no
dragging, no snap zones. Repeating a shortcut cycles through a configurable
set of sizes.

## Features

- Global shortcuts to snap the focused window to a side, a corner, the top,
  the bottom, or the center of its monitor.
- Repeating a shortcut steps through a size cycle (defaults to 1/2, 2/3, 1/3,
  2/7 of the work area).
- Maximize and restore, and moving a window to the next or previous monitor.
- A tray icon with a Settings window for changing any shortcut, the size
  cycle, and whether Wectangle launches at startup.
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

Running the executable a second time brings up the Settings window of the
already running instance instead of starting a second copy.

### Installing it for everyday use

Copy `target\release\wectangle.exe` somewhere permanent, for example
`%LOCALAPPDATA%\Programs\Wectangle`, run it from there, and turn on "Launch
at startup" from the tray menu or the Settings window.

### Enabling the pre-commit hook

This repository ships a hook that checks formatting and rejects em or en
dashes in `src/*.rs`. After cloning, enable it once with:

```
git config core.hooksPath .githooks
```

## Configuration

Settings are stored at `%APPDATA%\Wectangle\config.json`. It holds the size
cycle and the shortcut assigned to each action. Edit it through the Settings
window; a hand-edited file that fails to parse falls back to defaults with a
notification, and is left untouched until you press Save.

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
  - **Magnifier**, while it is running: Ctrl+Alt+Arrows, F, Space, Enter.
  - **VS Code**: Ctrl+Alt+Up/Down add cursors.
  - **JetBrains IDEs**: Ctrl+Alt+Left/Right navigate back and forward.

  Change any of these from the Settings window if they conflict with how you
  work.
