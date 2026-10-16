# Wrecktangle

A Rectangle-style keyboard window manager for Windows, in Rust.

## Build and test

- Format: `cargo fmt`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Test: `cargo test`
- Build: `cargo build --release`

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
  `main.rs`, with Win32 code behind `#[cfg(windows)]` inside them, because CI
  runs `cargo test` on Linux without the `windows` crate.
- The Settings window keeps its layout and painting in `ui::view` (pure,
  tested on Linux); `settings/window.rs` and `settings/gfx.rs` only handle
  Win32 messages and draw its operations with Direct2D.
- Owner lookups (`src/owners.rs`) read hotkey fields only. Never read, log or
  store other keys from those files (Twinkle Tray `udpKey`, tokens in the
  Claude config).
- The app was called Wectangle before. That name stays on purpose in the
  migration code only: `LEGACY_DIR` (`config.rs`), `LEGACY_VALUE_NAME`
  (`startup.rs`) and `close_legacy_instance` (`main.rs`). Don't rename them.
- `assets/wrecktangle.ico` and `assets/wrecktangle-256.png` are generated:
  edit `assets/wrecktangle.svg` (40 px and up) or `assets/wrecktangle-16.svg`
  to `-32.svg` (pixel-snapped tray sizes), then run
  `cargo run --example make_icon`. A design change belongs in all five SVGs.

## Developer overrides

- `WRECKTANGLE_THEME=light|dark` forces the Settings and menu theme.
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
