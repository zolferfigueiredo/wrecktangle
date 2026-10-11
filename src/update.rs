use std::cmp::Ordering;
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;

use crate::lang;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};
#[cfg(windows)]
use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
#[cfg(windows)]
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
#[cfg(windows)]
use windows::Win32::System::Threading::{
    CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW, WaitForSingleObject,
};
#[cfg(windows)]
use windows::Win32::UI::Shell::ShellExecuteW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};
#[cfg(windows)]
use windows::core::{PCWSTR, PWSTR, w};

const DEFAULT_URL: &str =
    "https://api.github.com/repos/zolferfigueiredo/wrecktangle/releases/latest";
const URL_OVERRIDE_VAR: &str = "WRECKTANGLE_UPDATE_URL";

const BAD_REPLY: &str = "unexpected reply";
const NO_DOWNLOAD: &str = "no download link";
const BAD_VERSION: &str = "unreadable release version";
const TOO_LARGE: &str = "reply too large";
const NOT_STARTED: &str = "could not start the update check";
const UNREACHABLE: &str = "could not connect";
const BAD_URL: &str = "invalid update URL";
const SERVER_ANSWERED: &str = "server answered ";
const DOWNLOAD_FAILED: &str = "the download failed";
const BAD_CHECKSUM: &str = "the download was damaged";
const INSTALLER_FAILED: &str = "the installer did not finish";
const SUMS_NAME: &str = "SHA256SUMS";
pub const AUTO_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[cfg(windows)]
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
#[cfg(windows)]
const MAX_INSTALLER_BYTES: usize = 64 * 1024 * 1024;
/// The installer closes this app before it installs, so a wait that ends
/// with the app still running means the update did not happen.
#[cfg(windows)]
const INSTALLER_WAIT_MS: u32 = 2 * 60 * 1000;
// The installer's AppId (installer/wrecktangle.iss) names this key.
#[cfg(windows)]
const UNINSTALL_KEY: PCWSTR = w!(
    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{5FC369EA-4EC3-4B23-9F2C-DAA462C9BD79}_is1"
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Checking,
    UpToDate {
        checked_at: u64,
    },
    /// `url` is the installer, or the release page when the release has no
    /// installer for this architecture. `error` is why the last update
    /// attempt failed, so the next one waits for a click.
    Available {
        version: String,
        url: String,
        sums_url: Option<String>,
        error: Option<String>,
    },
    Downloading {
        version: String,
    },
    Installing {
        version: String,
    },
    NoReleases,
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn state() -> State {
    STATE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn set_state(state: State) {
    *STATE.lock().unwrap_or_else(PoisonError::into_inner) = state;
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn status_line(state: &State) -> String {
    let version = current_version();
    match state {
        State::Idle => lang::format("about.version", &[("version", version)]),
        State::Checking => lang::t("status.checking"),
        State::UpToDate { .. } => format!(
            "{} \u{b7} {}",
            lang::format("about.version", &[("version", version)]),
            lang::t("status.up_to_date")
        ),
        State::Available {
            version,
            error: Some(reason),
            ..
        } => lang::format(
            "status.install_failed",
            &[("version", version), ("reason", &failure_text(reason))],
        ),
        State::Available { version, .. } => {
            lang::format("status.version_available", &[("version", version)])
        }
        State::Downloading { version } => {
            lang::format("status.downloading", &[("version", version)])
        }
        State::Installing { version } => lang::format("status.installing", &[("version", version)]),
        State::NoReleases => lang::t("status.no_releases"),
        State::Failed(message) => {
            lang::format("status.failed", &[("reason", &failure_text(message))])
        }
    }
}

pub fn status_detail(state: &State) -> String {
    match state {
        State::Idle => String::new(),
        State::UpToDate { .. } => lang::t("status.up_to_date"),
        other => status_line(other),
    }
}

fn failure_text(message: &str) -> String {
    let key = match message {
        BAD_REPLY => "failure.bad_reply",
        NO_DOWNLOAD => "failure.no_download",
        BAD_VERSION => "failure.bad_version",
        TOO_LARGE => "failure.too_large",
        NOT_STARTED => "failure.not_started",
        UNREACHABLE => "failure.unreachable",
        BAD_URL => "failure.bad_url",
        DOWNLOAD_FAILED => "failure.download",
        BAD_CHECKSUM => "failure.checksum",
        INSTALLER_FAILED => "failure.installer",
        other => {
            return match other.strip_prefix(SERVER_ANSWERED) {
                Some(code) => lang::format("failure.server", &[("code", code)]),
                None => other.to_string(),
            };
        }
    };
    lang::t(key)
}

/// A clock set back past the last check counts as due, so a wrong clock
/// cannot suppress checks indefinitely.
pub fn auto_check_due(last_check: u64, now: u64) -> bool {
    last_check > now || now - last_check >= AUTO_CHECK_INTERVAL_SECS
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<String>,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let core =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        core.then_with(|| match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => cmp_pre_release(a, b),
        })
    }
}

fn cmp_pre_release(a: &str, b: &str) -> Ordering {
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

pub fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim();
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    let text = text.split('+').next()?;
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_string())),
        Some(_) => return None,
        None => (text, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Version {
        major,
        minor,
        patch,
        pre,
    })
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub sums_url: Option<String>,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    #[serde(default)]
    browser_download_url: Option<String>,
}

/// The architecture this copy was built for, as release files name it, so a
/// 32-bit copy updates to the 32-bit installer.
const ARCH: &str = if cfg!(target_arch = "x86") {
    "x86"
} else {
    "x64"
};

/// The installer a release publishes for this architecture, named after the
/// release's version.
fn installer_name(tag: &str) -> String {
    format!(
        "Wrecktangle-{}-{ARCH}-setup.exe",
        tag.trim_start_matches('v')
    )
}

pub fn parse_release(json: &str) -> Result<Release, &'static str> {
    let release: ReleaseJson = serde_json::from_str(json).map_err(|_| BAD_REPLY)?;
    let asset_url = |name: &str| {
        release
            .assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .and_then(|a| a.browser_download_url.clone())
            .filter(|u| is_openable_url(u))
    };
    let url = asset_url(&installer_name(&release.tag_name))
        .or(release.html_url.clone())
        .filter(|u| is_openable_url(u))
        .ok_or(NO_DOWNLOAD)?;
    let sums_url = asset_url(SUMS_NAME);
    Ok(Release {
        tag: release.tag_name,
        url,
        sums_url,
    })
}

/// Only web links are handed to the shell, never a file path or other
/// scheme from a server reply.
pub fn is_openable_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// What an update needs to install itself: the installer and the
/// SHA256SUMS that vouches for it. None when the release only offers its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub version: String,
    pub file: String,
    pub url: String,
    pub sums_url: String,
}

pub fn download_for(state: &State) -> Option<Download> {
    let State::Available {
        version,
        url,
        sums_url: Some(sums_url),
        ..
    } = state
    else {
        return None;
    };
    let file = installer_name(version);
    let named = url
        .rsplit('/')
        .next()
        .is_some_and(|last| last.eq_ignore_ascii_case(&file));
    named.then(|| Download {
        version: version.clone(),
        file,
        url: url.clone(),
        sums_url: sums_url.clone(),
    })
}

/// The SHA-256 that SHA256SUMS (`sha256sum` output) lists for `file`, in
/// lowercase hex.
pub fn expected_sha256(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        let valid = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit());
        (valid && name.eq_ignore_ascii_case(file)).then(|| hash.to_ascii_lowercase())
    })
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether two folder paths are the same folder, as Windows compares them.
pub fn same_dir(a: &str, b: &str) -> bool {
    let normal = |p: &str| {
        p.trim()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let a = normal(a);
    !a.is_empty() && a == normal(b)
}

pub fn evaluate(status: u16, body: &str, current: &str, now: u64) -> State {
    match status {
        404 => State::NoReleases,
        200 => {
            let release = match parse_release(body) {
                Ok(release) => release,
                Err(message) => return State::Failed(message.to_string()),
            };
            let Some(latest) = parse_version(&release.tag) else {
                return State::Failed(BAD_VERSION.to_string());
            };
            if is_newer(&release.tag, current) {
                State::Available {
                    version: latest.to_string(),
                    url: release.url,
                    sums_url: release.sums_url,
                    error: None,
                }
            } else {
                State::UpToDate { checked_at: now }
            }
        }
        other => State::Failed(format!("{SERVER_ANSWERED}{other}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

/// Plain http is accepted only when `allow_http` is set, which is the case
/// only for the `WRECKTANGLE_UPDATE_URL` override.
pub fn parse_url(url: &str, allow_http: bool) -> Option<Url> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if allow_http && let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        return None;
    };
    let rest = rest.split('#').next()?;
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(split);
    if authority.is_empty() || authority.contains(['@', '[', ']']) {
        return None;
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok().filter(|p| *p != 0)?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return None;
    }
    let path = if tail.is_empty() {
        "/".to_string()
    } else if tail.starts_with('?') {
        format!("/{tail}")
    } else {
        tail.to_string()
    };
    Some(Url {
        secure,
        host: host.to_string(),
        port,
        path,
    })
}

fn url_override() -> Option<String> {
    std::env::var(URL_OVERRIDE_VAR)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn endpoint() -> Result<Url, String> {
    match url_override() {
        Some(value) => parse_url(&value, true)
            .ok_or_else(|| format!("{URL_OVERRIDE_VAR} is not a valid http or https URL")),
        None => parse_url(DEFAULT_URL, false).ok_or_else(|| BAD_URL.to_string()),
    }
}

#[cfg(windows)]
pub fn auto_enabled() -> bool {
    crate::app::current_config().check_updates
}

#[cfg(windows)]
pub fn set_auto(enabled: bool) {
    crate::app::update_config(|config| config.check_updates = enabled);
}

#[cfg(windows)]
pub fn auto_install_enabled() -> bool {
    crate::app::current_config().install_updates
}

#[cfg(windows)]
pub fn set_auto_install(enabled: bool) {
    crate::app::update_config(|config| config.install_updates = enabled);
}

#[cfg(windows)]
pub fn check_now() {
    start_check(false);
}

#[cfg(windows)]
pub fn check_auto() {
    start_check(true);
}

/// Whether the available update can install itself: this is the installed
/// copy (a portable or dev copy is never replaced) and the release has an
/// installer with checksums.
#[cfg(windows)]
pub fn can_install() -> bool {
    download_for(&state()).is_some() && is_installed_copy()
}

/// Whether to install the available update without a click: the switch is
/// on and the last attempt did not fail.
#[cfg(windows)]
pub fn should_install_now() -> bool {
    matches!(state(), State::Available { error: None, .. })
        && auto_install_enabled()
        && can_install()
}

/// Downloads the installer, checks it against SHA256SUMS and runs it
/// silently; the installer closes this app, installs and opens the new
/// version. Without an installable update this opens the download in the
/// browser, as before.
#[cfg(windows)]
pub fn install() {
    let current = state();
    let Some(download) = download_for(&current).filter(|_| is_installed_copy()) else {
        if let State::Available { url, .. } = current {
            open_url(&url);
        }
        return;
    };
    set_state(State::Downloading {
        version: download.version.clone(),
    });
    let hwnd = crate::app::main_hwnd().0 as isize;
    post_update_done(hwnd);
    let spawned = std::thread::Builder::new()
        .name("update-install".to_string())
        .spawn(move || {
            if let Err(reason) = download_and_run(&download, hwnd) {
                set_state(State::Available {
                    version: download.version.clone(),
                    url: download.url.clone(),
                    sums_url: Some(download.sums_url.clone()),
                    error: Some(reason),
                });
            }
            post_update_done(hwnd);
        });
    if spawned.is_err() {
        set_state(current);
    }
}

#[cfg(windows)]
fn download_and_run(download: &Download, hwnd: isize) -> Result<(), String> {
    let installer = fetch_file(&download.url, MAX_INSTALLER_BYTES)?;
    let sums = fetch_file(&download.sums_url, MAX_BODY_BYTES)?;
    let expected =
        expected_sha256(&String::from_utf8_lossy(&sums), &download.file).ok_or(BAD_CHECKSUM)?;
    if sha256(&installer).as_deref() != Some(expected.as_str()) {
        return Err(BAD_CHECKSUM.to_string());
    }

    let folder = downloads_dir();
    std::fs::create_dir_all(&folder).map_err(|_| DOWNLOAD_FAILED.to_string())?;
    let path = folder.join(&download.file);
    std::fs::write(&path, &installer).map_err(|_| DOWNLOAD_FAILED.to_string())?;

    set_state(State::Installing {
        version: download.version.clone(),
    });
    post_update_done(hwnd);
    let process = start_installer(&path)?;
    unsafe {
        WaitForSingleObject(process, INSTALLER_WAIT_MS);
        let _ = CloseHandle(process);
    }
    Err(INSTALLER_FAILED.to_string())
}

#[cfg(windows)]
fn downloads_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("Wrecktangle")
}

/// Removes installers left from earlier updates.
#[cfg(windows)]
pub fn clean_downloads() {
    let _ = std::fs::remove_dir_all(downloads_dir());
}

#[cfg(windows)]
fn start_installer(path: &std::path::Path) -> Result<HANDLE, String> {
    let mut command = to_wide(&format!(
        "\"{}\" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-",
        path.display()
    ));
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            PROCESS_CREATION_FLAGS(0),
            None,
            PCWSTR::null(),
            &startup,
            &mut info,
        )
        .map_err(|_| INSTALLER_FAILED.to_string())?;
        let _ = CloseHandle(info.hThread);
    }
    Ok(info.hProcess)
}

#[cfg(windows)]
fn post_update_done(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            crate::app::WM_APP_UPDATE_DONE,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

/// Whether this exe is the copy the installer put in place, by the
/// uninstall entry's InstallLocation.
#[cfg(windows)]
pub fn is_installed_copy() -> bool {
    let Some(location) = install_location() else {
        return false;
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.display().to_string()))
        .is_some_and(|dir| same_dir(&location, &dir))
}

#[cfg(windows)]
fn install_location() -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            w!("InstallLocation"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            Some(&mut len),
        )
    };
    if err.0 != 0 {
        return None;
    }
    let chars = (len as usize / 2).min(buf.len());
    Some(
        String::from_utf16_lossy(&buf[..chars])
            .trim_end_matches('\0')
            .to_string(),
    )
}

#[cfg(windows)]
fn sha256(data: &[u8]) -> Option<String> {
    let mut hash = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut hash) };
    status.is_ok().then(|| to_hex(&hash))
}

/// Downloads a release file. Release links are https; plain http is
/// accepted only with the `WRECKTANGLE_UPDATE_URL` override, as for the
/// check itself.
#[cfg(windows)]
fn fetch_file(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let url = parse_url(url, url_override().is_some()).ok_or(DOWNLOAD_FAILED)?;
    let agent = format!("Wrecktangle/{}", current_version());
    match fetch_bytes(&url, &agent, "Accept: application/octet-stream\r\n", limit)? {
        (200, body) => Ok(body),
        _ => Err(DOWNLOAD_FAILED.to_string()),
    }
}

#[cfg(windows)]
pub fn open_url(url: &str) {
    if !is_openable_url(url) {
        return;
    }
    let wide = to_wide(url);
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

// Runs on the UI thread. The worker posts the result back instead of
// touching any UI state itself.
#[cfg(windows)]
fn start_check(auto: bool) {
    {
        let mut guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
        if *guard == State::Checking {
            return;
        }
        *guard = State::Checking;
    }

    let hwnd = crate::app::main_hwnd().0 as isize;
    let spawned = std::thread::Builder::new()
        .name("update-check".to_string())
        .spawn(move || {
            let result = run_check();
            let checked_at = match &result {
                State::UpToDate { checked_at } => *checked_at,
                State::Available { .. } | State::NoReleases => now_unix(),
                _ => 0,
            };
            set_state(result);
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut core::ffi::c_void)),
                    crate::app::WM_APP_UPDATE_DONE,
                    WPARAM(auto as usize),
                    LPARAM(checked_at as isize),
                );
            }
        });
    if spawned.is_err() {
        set_state(State::Failed(NOT_STARTED.to_string()));
    }
}

#[cfg(windows)]
fn run_check() -> State {
    let url = match endpoint() {
        Ok(url) => url,
        Err(message) => return State::Failed(message),
    };
    let agent = format!("Wrecktangle/{}", current_version());
    match fetch(&url, &agent) {
        Ok((status, body)) => evaluate(status, &body, current_version(), now_unix()),
        Err(message) => State::Failed(message),
    }
}

#[cfg(windows)]
fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
struct Handle(*mut core::ffi::c_void);

#[cfg(windows)]
impl Handle {
    fn new(raw: *mut core::ffi::c_void) -> Result<Handle, String> {
        if raw.is_null() {
            Err(UNREACHABLE.to_string())
        } else {
            Ok(Handle(raw))
        }
    }
}

#[cfg(windows)]
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn fetch(url: &Url, agent: &str) -> Result<(u16, String), String> {
    let (status, body) = fetch_bytes(
        url,
        agent,
        "Accept: application/vnd.github+json\r\n",
        MAX_BODY_BYTES,
    )?;
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

// WinHTTP follows the redirect GitHub answers release downloads with.
#[cfg(windows)]
fn fetch_bytes(
    url: &Url,
    agent: &str,
    headers: &str,
    limit: usize,
) -> Result<(u16, Vec<u8>), String> {
    let unreachable = |_| UNREACHABLE.to_string();
    unsafe {
        let agent_w = to_wide(agent);
        let session = Handle::new(WinHttpOpen(
            PCWSTR(agent_w.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 15_000);

        let host_w = to_wide(&url.host);
        let connection = Handle::new(WinHttpConnect(
            session.0,
            PCWSTR(host_w.as_ptr()),
            url.port,
            0,
        ))?;

        let path_w = to_wide(&url.path);
        let flags = if url.secure {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let request = Handle::new(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        ))?;

        let headers: Vec<u16> = headers.encode_utf16().collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).map_err(unreachable)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(unreachable)?;

        let mut status: u32 = 0;
        let mut status_len = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut core::ffi::c_void),
            &mut status_len,
            std::ptr::null_mut(),
        )
        .map_err(unreachable)?;

        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut core::ffi::c_void,
                chunk.len() as u32,
                &mut read,
            )
            .map_err(unreachable)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > limit {
                return Err(TOO_LARGE.to_string());
            }
        }
        Ok((status as u16, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE_JSON: &str = r#"{
        "url": "https://api.github.com/repos/zolferfigueiredo/wrecktangle/releases/1",
        "html_url": "https://github.com/zolferfigueiredo/wrecktangle/releases/tag/v0.3.0",
        "tag_name": "v0.3.0",
        "name": "v0.3.0",
        "draft": false,
        "prerelease": false,
        "body": "notes",
        "assets": [
            {"name": "notes.txt", "browser_download_url": "https://github.com/x/releases/download/v0.3.0/notes.txt"},
            {"name": "Wrecktangle-0.2.2-x64-setup.exe", "browser_download_url": "https://github.com/x/releases/download/v0.3.0/Wrecktangle-0.2.2-x64-setup.exe"},
            {"name": "Wrecktangle-0.3.0-x86-setup.exe", "browser_download_url": "https://github.com/x/releases/download/v0.3.0/Wrecktangle-0.3.0-x86-setup.exe"},
            {"name": "Wrecktangle-0.3.0-x64-setup.exe", "browser_download_url": "https://github.com/x/releases/download/v0.3.0/Wrecktangle-0.3.0-x64-setup.exe"},
            {"name": "SHA256SUMS", "browser_download_url": "https://github.com/x/releases/download/v0.3.0/SHA256SUMS"}
        ]
    }"#;

    const SUMS_URL: &str = "https://github.com/x/releases/download/v0.3.0/SHA256SUMS";

    /// The installer RELEASE_JSON offers this build's architecture.
    fn expected_installer() -> String {
        format!("https://github.com/x/releases/download/v0.3.0/Wrecktangle-0.3.0-{ARCH}-setup.exe")
    }

    #[test]
    fn parse_version_accepts_plain_and_prefixed_versions() {
        let v = parse_version("v1.2.3").unwrap();
        assert_eq!(v.to_string(), "1.2.3");
        assert_eq!(parse_version("0.10.0").unwrap().to_string(), "0.10.0");
        assert_eq!(parse_version("1.2.3+build.5").unwrap().to_string(), "1.2.3");
        assert_eq!(
            parse_version("1.2.3-rc.1").unwrap().to_string(),
            "1.2.3-rc.1"
        );
    }

    #[test]
    fn parse_version_rejects_malformed_input() {
        for bad in [
            "", "v", "1", "1.2", "1.2.3.4", "a.b.c", "1.2.x", "1.2.3-", "latest",
        ] {
            assert!(parse_version(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn versions_compare_numerically_not_textually() {
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("v0.2.1", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
    }

    #[test]
    fn pre_releases_sort_below_their_release() {
        assert!(is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(is_newer("1.0.0-rc.2", "1.0.0-rc.1"));
        assert!(is_newer("1.0.0-rc.10", "1.0.0-rc.9"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0-beta.9"));
        assert!(is_newer("1.0.0-rc.1.1", "1.0.0-rc.1"));
    }

    #[test]
    fn unparseable_versions_are_never_newer() {
        assert!(!is_newer("latest", "0.2.0"));
        assert!(!is_newer("0.3.0", "dev"));
    }

    #[test]
    fn current_version_is_valid_semver() {
        assert!(parse_version(current_version()).is_some());
    }

    #[test]
    fn release_json_picks_the_installer_named_for_its_version() {
        let release = parse_release(RELEASE_JSON).unwrap();
        assert_eq!(release.tag, "v0.3.0");
        assert_eq!(release.url, expected_installer());
    }

    #[test]
    fn release_json_picks_the_checksums() {
        assert_eq!(
            parse_release(RELEASE_JSON).unwrap().sums_url.as_deref(),
            Some(SUMS_URL)
        );
        let without =
            r#"{"tag_name":"v0.3.0","html_url":"https://github.com/x/releases/tag/v0.3.0"}"#;
        assert_eq!(parse_release(without).unwrap().sums_url, None);
    }

    fn available(url: &str, sums_url: Option<&str>) -> State {
        State::Available {
            version: "0.3.0".to_string(),
            url: url.to_string(),
            sums_url: sums_url.map(str::to_string),
            error: None,
        }
    }

    #[test]
    fn an_update_installs_from_its_installer_with_checksums() {
        assert_eq!(
            download_for(&available(&expected_installer(), Some(SUMS_URL))),
            Some(Download {
                version: "0.3.0".to_string(),
                file: format!("Wrecktangle-0.3.0-{ARCH}-setup.exe"),
                url: expected_installer(),
                sums_url: SUMS_URL.to_string(),
            })
        );
        assert_eq!(download_for(&available(&expected_installer(), None)), None);
        let page = "https://github.com/x/releases/tag/v0.3.0";
        assert_eq!(download_for(&available(page, Some(SUMS_URL))), None);
        assert_eq!(download_for(&State::Checking), None);
    }

    #[test]
    fn checksums_are_read_from_sha256sum_output() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums = format!(
            "{a}  Wrecktangle-0.3.0-x64.zip\n{b} *Wrecktangle-0.3.0-x64-setup.exe\nnot a line\n"
        );
        assert_eq!(
            expected_sha256(&sums, "Wrecktangle-0.3.0-x64-setup.exe"),
            Some("b".repeat(64))
        );
        assert_eq!(
            expected_sha256(&sums, "Wrecktangle-0.3.0-x64.zip"),
            Some(a.clone())
        );
        assert_eq!(expected_sha256(&sums, "other.exe"), None);
        assert_eq!(expected_sha256("abc  short.exe", "short.exe"), None);
    }

    #[test]
    fn hex_is_lowercase_and_two_digits_a_byte() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
    }

    #[test]
    fn same_dir_ignores_case_slashes_and_a_trailing_separator() {
        assert!(same_dir(
            "C:\\Users\\Zolfe\\AppData\\Local\\Programs\\Wrecktangle\\",
            "c:\\users\\zolfe\\appdata\\local\\programs\\wrecktangle"
        ));
        assert!(same_dir("C:/Apps/Wrecktangle", "C:\\Apps\\Wrecktangle"));
        assert!(!same_dir(
            "C:\\Users\\Zolfe\\AppData\\Local\\Programs\\Wrecktangle",
            "C:\\dev\\wrecktangle\\target\\release"
        ));
        assert!(!same_dir("", ""));
    }

    #[test]
    fn release_json_falls_back_to_the_release_page() {
        let json = r#"{"tag_name":"v0.3.0","html_url":"https://github.com/x/releases/tag/v0.3.0","assets":[{"name":"other.zip","browser_download_url":"https://example.com/other.zip"}]}"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.url, "https://github.com/x/releases/tag/v0.3.0");

        let no_assets =
            r#"{"tag_name":"v0.3.0","html_url":"https://github.com/x/releases/tag/v0.3.0"}"#;
        assert_eq!(parse_release(no_assets).unwrap().url, release.url);
    }

    #[test]
    fn installer_name_follows_the_tag() {
        assert_eq!(
            installer_name("v0.3.0"),
            format!("Wrecktangle-0.3.0-{ARCH}-setup.exe")
        );
        assert_eq!(
            installer_name("1.0.0"),
            format!("Wrecktangle-1.0.0-{ARCH}-setup.exe")
        );
        let expected = if cfg!(target_pointer_width = "32") {
            "x86"
        } else {
            "x64"
        };
        assert_eq!(ARCH, expected);
    }

    #[test]
    fn release_json_without_tag_or_link_is_rejected() {
        assert!(parse_release("{}").is_err());
        assert!(parse_release("not json").is_err());
        assert!(parse_release(r#"{"tag_name":"v1.0.0"}"#).is_err());
    }

    #[test]
    fn release_json_ignores_non_web_links() {
        let json = r#"{"tag_name":"v0.3.0","html_url":"file:///c:/windows/system32/calc.exe","assets":[]}"#;
        assert!(parse_release(json).is_err());
    }

    #[test]
    fn evaluate_reports_a_newer_release_as_available() {
        let state = evaluate(200, RELEASE_JSON, "0.2.0", 1000);
        assert_eq!(
            state,
            State::Available {
                version: "0.3.0".to_string(),
                url: expected_installer(),
                sums_url: Some(SUMS_URL.to_string()),
                error: None,
            }
        );
    }

    #[test]
    fn evaluate_reports_the_same_or_older_release_as_up_to_date() {
        assert_eq!(
            evaluate(200, RELEASE_JSON, "0.3.0", 1000),
            State::UpToDate { checked_at: 1000 }
        );
        assert_eq!(
            evaluate(200, RELEASE_JSON, "1.0.0", 7),
            State::UpToDate { checked_at: 7 }
        );
    }

    #[test]
    fn evaluate_treats_404_as_no_releases_yet() {
        assert_eq!(
            evaluate(404, r#"{"message":"Not Found"}"#, "0.2.0", 1),
            State::NoReleases
        );
    }

    #[test]
    fn evaluate_turns_other_statuses_and_bad_bodies_into_failures() {
        assert!(matches!(evaluate(403, "", "0.2.0", 1), State::Failed(m) if m.contains("403")));
        assert!(matches!(evaluate(500, "", "0.2.0", 1), State::Failed(_)));
        assert!(matches!(
            evaluate(200, "oops", "0.2.0", 1),
            State::Failed(_)
        ));
        let odd_tag = r#"{"tag_name":"nightly","html_url":"https://github.com/x/releases"}"#;
        assert!(matches!(
            evaluate(200, odd_tag, "0.2.0", 1),
            State::Failed(_)
        ));
    }

    #[test]
    fn auto_check_is_due_after_a_day_or_when_the_clock_went_back() {
        let day = AUTO_CHECK_INTERVAL_SECS;
        assert!(auto_check_due(0, 1_000_000));
        assert!(!auto_check_due(1_000_000, 1_000_000 + day - 1));
        assert!(auto_check_due(1_000_000, 1_000_000 + day));
        assert!(auto_check_due(2_000_000, 1_000_000));
    }

    #[test]
    fn status_line_covers_every_state() {
        let version = current_version();
        assert_eq!(status_line(&State::Idle), format!("Version {version}"));
        assert_eq!(status_line(&State::Checking), "Checking\u{2026}");
        assert_eq!(
            status_line(&State::UpToDate { checked_at: 5 }),
            format!("Version {version} \u{b7} Up to date")
        );
        assert_eq!(
            status_line(&State::Available {
                version: "9.9.9".to_string(),
                url: "https://example.com".to_string(),
                sums_url: None,
                error: None,
            }),
            "Version 9.9.9 is available"
        );
        assert_eq!(
            status_line(&State::Available {
                version: "9.9.9".to_string(),
                url: "https://example.com".to_string(),
                sums_url: None,
                error: Some(BAD_CHECKSUM.to_string()),
            }),
            "Couldn't update to 9.9.9: the download was damaged"
        );
        assert_eq!(
            status_line(&State::Downloading {
                version: "9.9.9".to_string()
            }),
            "Downloading version 9.9.9\u{2026}"
        );
        assert_eq!(
            status_line(&State::Installing {
                version: "9.9.9".to_string()
            }),
            "Installing version 9.9.9\u{2026}"
        );
        assert_eq!(status_line(&State::NoReleases), "No releases published yet");
        assert_eq!(
            status_line(&State::Failed("could not connect".to_string())),
            "Update check failed: could not connect"
        );
    }

    #[test]
    fn status_detail_leaves_out_the_current_version() {
        assert_eq!(status_detail(&State::Idle), "");
        assert_eq!(
            status_detail(&State::UpToDate { checked_at: 5 }),
            "Up to date"
        );
        assert_eq!(status_detail(&State::Checking), "Checking\u{2026}");
        assert_eq!(
            status_detail(&State::NoReleases),
            "No releases published yet"
        );
    }

    #[test]
    fn urls_parse_into_host_port_and_path() {
        assert_eq!(
            parse_url(DEFAULT_URL, false),
            Some(Url {
                secure: true,
                host: "api.github.com".to_string(),
                port: 443,
                path: "/repos/zolferfigueiredo/wrecktangle/releases/latest".to_string(),
            })
        );
        assert_eq!(
            parse_url("http://localhost:8000/releases/latest?x=1#frag", true),
            Some(Url {
                secure: false,
                host: "localhost".to_string(),
                port: 8000,
                path: "/releases/latest?x=1".to_string(),
            })
        );
        assert_eq!(parse_url("http://127.0.0.1", true).unwrap().path, "/");
        assert_eq!(parse_url("http://127.0.0.1", true).unwrap().port, 80);
        assert_eq!(parse_url("https://h?q=1", false).unwrap().path, "/?q=1");
    }

    #[test]
    fn plain_http_needs_the_override_flag() {
        assert_eq!(parse_url("http://localhost:8000/x", false), None);
        assert!(parse_url("http://localhost:8000/x", true).is_some());
        assert!(parse_url("https://localhost:8443/x", false).is_some());
    }

    #[test]
    fn malformed_urls_are_rejected() {
        for bad in [
            "",
            "ftp://example.com/x",
            "file:///c:/x",
            "https://",
            "https:///path",
            "https://user@example.com/x",
            "https://example.com:0/x",
            "https://example.com:notaport/x",
            "https://[::1]/x",
            "api.github.com/x",
        ] {
            assert_eq!(parse_url(bad, true), None, "{bad}");
        }
    }

    #[test]
    fn only_web_links_are_openable() {
        assert!(is_openable_url("https://github.com/x/releases"));
        assert!(is_openable_url("http://localhost:8000/wrecktangle.exe"));
        assert!(!is_openable_url("file:///c:/windows/system32/calc.exe"));
        assert!(!is_openable_url("c:\\windows\\system32\\calc.exe"));
        assert!(!is_openable_url("ms-settings:"));
    }

    #[cfg(windows)]
    fn serve_once(status_line: &str, body: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let status_line = status_line.to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (port, server)
    }

    #[cfg(windows)]
    #[test]
    fn fetch_sends_the_github_headers_and_returns_status_and_body() {
        let (port, server) = serve_once("200 OK", RELEASE_JSON);
        let url = parse_url(
            &format!("http://127.0.0.1:{port}/repos/x/y/releases/latest"),
            true,
        )
        .unwrap();
        let (status, body) = fetch(&url, "Wrecktangle/test").unwrap();
        let request = server.join().unwrap().to_ascii_lowercase();
        assert_eq!(status, 200);
        assert_eq!(body, RELEASE_JSON);
        assert!(request.starts_with("get /repos/x/y/releases/latest http/1.1"));
        assert!(request.contains("user-agent: wrecktangle/test"));
        assert!(request.contains("accept: application/vnd.github+json"));
        assert_eq!(
            evaluate(status, &body, "0.2.0", 1),
            State::Available {
                version: "0.3.0".to_string(),
                url: expected_installer(),
                sums_url: Some(SUMS_URL.to_string()),
                error: None,
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn fetch_returns_a_404_as_a_status_not_an_error() {
        let (port, server) = serve_once("404 Not Found", r#"{"message":"Not Found"}"#);
        let url = parse_url(&format!("http://127.0.0.1:{port}/x"), true).unwrap();
        let (status, body) = fetch(&url, "Wrecktangle/test").unwrap();
        server.join().unwrap();
        assert_eq!(evaluate(status, &body, "0.2.0", 1), State::NoReleases);
    }

    #[cfg(windows)]
    #[test]
    fn fetch_reports_an_unreachable_server_as_an_error() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let url = parse_url(&format!("http://127.0.0.1:{port}/x"), true).unwrap();
        assert_eq!(
            fetch(&url, "Wrecktangle/test"),
            Err("could not connect".to_string())
        );
    }
}
