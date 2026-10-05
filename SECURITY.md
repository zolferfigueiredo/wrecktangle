# Security policy

## Supported versions

Only the [latest release](https://github.com/zolferfigueiredo/wrecktangle/releases/latest)
gets security fixes. Wrecktangle tells you when a new one is out, and
updating is the way to get a fix.

## Reporting a vulnerability

Please don't report security problems in public issues. Email
**contact@zolfer.com** instead, with:

- what the problem is and what an attacker could do with it;
- the steps to reproduce it, and the Wrecktangle and Windows versions;
- a proof of concept, if you have one.

You'll get a reply as soon as possible. Once the problem is confirmed, a fix is
released and the report is credited in the release notes, unless you'd rather
stay anonymous. Please give a fix a chance to ship before you disclose the
problem publicly.

## What Wrecktangle does that matters for security

- It runs without admin rights and can't move windows of apps that run as
  administrator.
- It installs a low-level keyboard hook only to take over shortcuts that
  another app already owns, and while you record a shortcut in Settings. Keys
  are only compared with your shortcuts, on your PC: nothing you type is kept
  or sent anywhere, apart from the shortcut you record, which is saved in your
  settings.
- To name the app that owns a shortcut, it reads the hotkey settings of a few
  known apps (PowerToys, Twinkle Tray, the NVIDIA overlay, Claude). It reads
  only the hotkey fields and ignores everything else in those files.
- Its only network access is the update check: an HTTPS request to the GitHub
  releases API for this repository. It never downloads or installs updates by
  itself; a new version opens in your browser.
- Its settings are stored in `%APPDATA%\Wrecktangle\config.json`.
