# Wectangle

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
