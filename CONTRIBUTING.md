# Contributing to Wrecktangle

Thanks for helping. Bug reports, ideas, translations and code are all welcome.

- **Found a bug or want a feature?** Open an
  [issue](https://github.com/zolferfigueiredo/wrecktangle/issues/new/choose).
  For anything bigger than a small fix, please open one before writing code,
  so we can agree on the approach first.
- **Found a security problem?** Don't open an issue; follow
  [SECURITY.md](SECURITY.md).

Everyone taking part is expected to follow the
[code of conduct](CODE_OF_CONDUCT.md).

## Setting up

You need Windows 10 or 11, the Rust toolchain pinned in `rust-toolchain.toml`
(rustup installs it on first build), the MSVC "Desktop development with C++"
build tools and a Windows 10 SDK.

```
git clone https://github.com/zolferfigueiredo/wrecktangle.git
cd wrecktangle
git config core.hooksPath .githooks
```

The hook refuses commits on `main`, runs `cargo fmt --check` and rejects em
and en dashes in `src/*.rs`.

`run.bat` builds whatever changed and restarts Wrecktangle; `build.bat` only
builds. Only one Wrecktangle runs at a time (starting a second one just opens
Settings in the first), so both scripts close a running copy first.

Two environment variables help while testing:

- `WRECKTANGLE_THEME=light` or `dark` forces the theme of Settings and menus.
- `WRECKTANGLE_UPDATE_URL` points the update check at another server, such as
  a local one serving a fake release.

## Before opening a pull request

```
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same checks on Linux, cross-compiling for Windows. Code that needs
the `windows` crate goes behind `#[cfg(windows)]`, so the platform independent
parts (config, layout, translations, the Settings layout in `src/ui`) build
and run their tests there.

## Code guidelines

- Window geometry uses the visible frame from `DWMWA_EXTENDED_FRAME_BOUNDS`,
  never the raw `GetWindowRect`, which includes the invisible resize border.
- Never hold a `RefCell` borrow across a Win32 call that can re-enter a window
  procedure (`SetWindowPos`, `ShowWindow`, `SendMessage`, `TrackPopupMenu`,
  `RegisterHotKey` and the like). Copy what you need, drop the borrow, then
  make the call.
- The keyboard hooks run on the UI thread, so keep them short.
- When reading another app's settings to name the owner of a shortcut, read
  hotkey fields only.
- No em or en dashes in Rust sources or in any text a person reads.
- `assets/wrecktangle.ico` and `assets/wrecktangle-256.png` are generated:
  edit the SVGs in `assets/` and run `cargo run --example make_icon`.

`CLAUDE.md` lists these rules in full.

## Translations

Every string the app shows lives in `lang/<code>.json`, one file per language
(German, English, Spanish, French, Italian, Polish, Brazilian Portuguese,
Russian, Ukrainian, Chinese, Japanese and Korean). When you add or change a
string:

- add it to all twelve files, keeping the keys in alphabetical order;
- keep the `{placeholders}` the English text uses;
- don't use em or en dashes or `...` (use the `…` character).

`cargo test` checks all of this. If you speak one of these languages and spot
something that reads badly, a fix is very welcome.

## Branches, commits and pull requests

- Branch from `main` and name the branch after the change: `feature/`, `fix/`,
  `docs/` or `chore/`, then a short kebab-case description, for example
  `fix/tray-icon-after-explorer-restart`.
- Write commit messages in the
  [Conventional Commits](https://www.conventionalcommits.org) style:
  `feat: ...`, `fix: ...`, `docs: ...`, `chore: ...`, `test: ...`.
- Open the pull request against `main` and fill in the template. CI has to
  pass before it is merged.

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE), like the rest of the project.
