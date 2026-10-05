# Wrecktangle

A Rectangle-style keyboard window manager for Windows, in Rust.

## Build and test

- Format: `cargo fmt`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Test: `cargo test`
- Build: `cargo build --release`

## Invariants

- All window geometry uses the visible DWM frame
  (`DWMWA_EXTENDED_FRAME_BOUNDS`), never `GetWindowRect`'s raw rect, which
  includes the invisible resize border.
- A `thread_local` `RefCell` must never be borrowed across a Win32 call that
  can re-enter a window procedure: `SetWindowPos`, `ShowWindow`,
  `SendMessage` to a control, `MessageBoxW`, `TrackPopupMenu`,
  `DestroyWindow`, `RegisterHotKey`/`UnregisterHotKey`. Copy the data out,
  drop the borrow, then make the call.
- No em or en dashes anywhere in `src/*.rs` (enforced by the pre-commit
  hook).
- The takeover keyboard hook (`src/takeover.rs`) runs on the UI thread, so it
  must stay fast and must not touch `app.rs`'s `STATE`: it keeps its own
  `thread_local` state and only posts a message to the main window.
- Pure modules (`config`, `lang`, `layout`, `owners`, `shortcut`,
  `takeover`, `theme`, `ui`, `update`) are declared unconditionally in
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

Bump `version` in `Cargo.toml` and in the README's release badge (the repo
is private, so shields.io can't read releases; CI checks the two match), tag
`v<version>` and push the tag; the
Release workflow publishes the installer and zip, versioned and under fixed
names, plus `SHA256SUMS` and `latest.json`. The update check looks for the
fixed-name `Wrecktangle-x64-setup.exe` (`ASSET_NAME` in `src/update.rs`), so
keep that name and the workflow in sync. The installer is
`installer/wrecktangle.iss`; its Run value name and `AppMutex` must match
`src/startup.rs` and `src/main.rs`.
