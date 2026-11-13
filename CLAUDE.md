# Wrecktangle

A Rectangle-style keyboard window manager for Windows and Linux (X11), in
Rust.

## Build and test

- Format: `cargo fmt`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Test: `cargo test`
- Build: `cargo build --release`
- Linux: `./run.sh` builds and (re)starts it; `scripts/install-linux.sh`
  installs it into `~/.local`.

## Invariants

- All window geometry uses the visible DWM frame
  (`DWMWA_EXTENDED_FRAME_BOUNDS`) minus the border Windows 11 draws inside it
  (`DWMWA_VISIBLE_FRAME_BORDER_THICKNESS`), so that border lands outside the
  tile. Never `GetWindowRect`'s raw rect, which includes the invisible resize
  border.
- A `thread_local` `RefCell` must never be borrowed across a Win32 call that
  can re-enter a window procedure: `SetWindowPos`, `ShowWindow`,
  `SendMessage` to a control, `MessageBoxW`, `TrackPopupMenu`,
  `DestroyWindow`, `RegisterHotKey`/`UnregisterHotKey`, and the Task
  Scheduler COM calls in `src/startup.rs` (out-of-process calls from an STA
  pump messages). Copy the data out, drop the borrow, then make the call.
- No em or en dashes anywhere in `src/*.rs` (enforced by the pre-commit
  hook).
- The takeover keyboard hook (`src/takeover.rs`) runs on the UI thread, so it
  must stay fast and must not touch `app.rs`'s `STATE`: it keeps its own
  `thread_local` state and only posts a message to the main window.
- Pure modules (`config`, `lang`, `layout`, `owners`, `shortcut`,
  `startup`, `takeover`, `theme`, `ui`, `update`) are declared unconditionally in
  `main.rs`, with Win32 code behind `#[cfg(windows)]` and Linux code behind
  `#[cfg(target_os = "linux")]` (or `not(windows)`) inside them, because CI
  runs `cargo test` on Linux without the `windows` crate. `xfce` is pure too,
  declared for `any(not(windows), test)`. A pure item only one system uses
  needs such a `cfg`, or Clippy fails the other system's build on dead code.
- The Settings window keeps its layout and painting in `ui::view`, what the
  pointer and keys do in `ui::session`, and the model's rows in `ui::model`
  (all pure, tested on Linux). `settings/window.rs` and `settings/gfx.rs`
  (Win32, Direct2D) and `linux/settings/window.rs` and
  `linux/settings/gfx.rs` (X11, tiny-skia, cosmic-text) only translate
  events and draw the operations.
- The Linux build lives in `src/linux/` and answers to the same module names
  as the Windows one (`crate::app` is `linux::app` there), so shared code
  such as `update` calls the same functions on both.
- On X11 the visible frame is the client window plus `_NET_FRAME_EXTENTS`
  (the window manager's decorations) minus `_GTK_FRAME_EXTENTS` (a CSD
  window's invisible shadow), as `linux/window_ops.rs` `visible_from` does.
  Never tile the client rect alone.
- On Linux only the main thread reads X events or touches app state. Other
  threads (tray, notifications, update check, timer, signals) post a
  ClientMessage with `x11::post`. The desktop theme watchers in
  `linux/desktop.rs` read their own X connection and the session bus, and
  reach an open Settings window with `settings::desktop_changed`. A process that messages a running copy
  (a second start, `--quit`) makes a round trip before it exits, or the X
  server can drop the message.
- The Xfce takeover (`linux/app.rs` `register_all`, `linux/xfconf.rs`)
  records each Xfce binding it removes in
  `~/.config/wrecktangle/xfce-takeover.json` before grabbing the chord, and
  puts it back on quit, SIGTERM/SIGHUP/SIGINT, X connection loss, the next
  start, and in `scripts/uninstall-linux.sh` (`--restore-xfce-shortcuts`).
  Ungrab before restoring, or Xfce can't grab the key back.
- `config.json` writes the Win key as "Win" on every system; only display
  text uses `shortcut::WIN_LABEL` ("Super" on Linux) through
  `shortcut::display` and `key_caps`.
- Owner lookups (`src/owners.rs`) read hotkey fields only. Never read, log or
  store other keys from those files (Twinkle Tray `udpKey`, tokens in the
  Claude config).
- The app was called Wectangle before. That name stays on purpose in the
  migration code only: `LEGACY_DIR` (`config.rs`), `LEGACY_VALUE_NAME`
  (`startup.rs`) and `close_legacy_instance` (`main.rs`). Don't rename them.
- `assets/wrecktangle.ico`, `assets/wrecktangle-256.png`, the Linux window
  icons `assets/wrecktangle-32.png` to `-64.png` and the Linux tray icons
  `assets/wrecktangle-tray-white-16.png` to `-tray-black-64.png` are
  generated: edit `assets/wrecktangle.svg` (40 px and up) or
  `assets/wrecktangle-16.svg` to `-32.svg` (pixel-snapped small sizes) for
  the logo, or `assets/wrecktangle-tray.svg` and `-tray-16.svg` to
  `-tray-32.svg` for the one-color tray icon (drawn black; the white one is
  made from it), then run `cargo run --example make_icon`. A design change
  belongs in all five SVGs of its set.
- The Theme setting (`theme::Theme`) picks the mode through
  `theme::current_mode`. Its System choice on Linux, and the accent there
  when Wrecktangle colors are off, come from `linux/desktop.rs`: the
  portal's `org.freedesktop.appearance` `color-scheme` and `accent-color`
  first, then the GTK theme named in XSETTINGS (`Net/ThemeName` containing
  "dark") and its gtk.css selection color.

## Developer overrides

- `WRECKTANGLE_THEME=light|dark` forces the Settings and menu theme, and
  the Linux tray icon's, over the Theme setting.
- `WRECKTANGLE_UPDATE_URL` replaces the GitHub releases API URL for the update
  check; plain `http://` is accepted only through it. Use it with a local
  server. A second `wrecktangle.exe` does not start: it only opens Settings in
  the running instance, so quit that one before trying a new build.

## Releasing

`release.bat <version>` (`scripts/release.ps1`) opens the bump pull request
into `dev`: `version` in `Cargo.toml`, `Cargo.lock`, and the README's release
badge and download links (CI checks they all match). Merging `dev` into `main`
then releases it: the Release workflow publishes any version on `main` with no
release yet, tagging the merged commit. `release.bat` with no version is the
by-hand way: it checks CI on `main`, then tags `v<version>` on `main` and
pushes the tag (the workflow refuses tags that aren't on `main`). Either way
the Release workflow publishes `Wrecktangle-<version>-<arch>-setup.exe` and
`Wrecktangle-<version>-<arch>.zip` for `x64` (64-bit) and `x86` (32-bit),
plus `SHA256SUMS` and `latest.json`. There are no unversioned copies. The
update check looks for the installer named after the release's tag and the
running build's architecture (`installer_name` and `ARCH` in
`src/update.rs`), so keep those names and the workflow in sync. The 32-bit
build differs in places (some Win32 structs are packed, so their fields
can't be borrowed); CI runs Clippy for it. The installer is
`installer/wrecktangle.iss`; its Run value name must match `src/startup.rs`.
It has no `AppMutex`: its `[Code]` quits a running copy itself (`WM_CLOSE` to
`WINDOW_CLASS_NAME` from `src/app.rs`, then waits on the mutex from
`src/main.rs`), so those names must match too.

Installed copies update themselves (`update::install`): the app downloads the
release's installer, checks it against the release's `SHA256SUMS`, and runs
it with `/VERYSILENT`. The installer closes the app and starts the new version
with `--updated`. A portable or dev copy (not in the installer's
`InstallLocation`) only opens the download in the browser.

Launch at startup is a per-user Task Scheduler logon task, because Explorer
starts Run-key apps late and one at a time. The installer's checkbox still
writes the Run value, and the app moves it to the task on launch
(`migrate_run_value`). The task's name (`task_name` in `src/startup.rs`) must
match the installer's `[UninstallRun]` entry, which deletes it.
