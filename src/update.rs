use std::cmp::Ordering;
use std::sync::{Mutex, PoisonError};

use serde::Deserialize;

use crate::lang;

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};
#[cfg(windows)]
use windows::Win32::UI::Shell::ShellExecuteW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};
#[cfg(windows)]
use windows::core::{PCWSTR, w};

const DEFAULT_URL: &str =
    "https://api.github.com/repos/zolferfigueiredo/wrecktangle/releases/latest";
const URL_OVERRIDE_VAR: &str = "WRECKTANGLE_UPDATE_URL";
const ASSET_NAME: &str = "wrecktangle.exe";

const BAD_REPLY: &str = "unexpected reply";
const NO_DOWNLOAD: &str = "no download link";
const BAD_VERSION: &str = "unreadable release version";
const TOO_LARGE: &str = "reply too large";
const NOT_STARTED: &str = "could not start the update check";
const UNREACHABLE: &str = "could not connect";
const BAD_URL: &str = "invalid update URL";
const SERVER_ANSWERED: &str = "server answered ";
pub const AUTO_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[cfg(windows)]
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Checking,
    UpToDate { checked_at: u64 },
    Available { version: String, url: String },
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
        State::Available { version, .. } => {
            lang::format("status.version_available", &[("version", version)])
        }
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

pub fn parse_release(json: &str) -> Result<Release, &'static str> {
    let release: ReleaseJson = serde_json::from_str(json).map_err(|_| BAD_REPLY)?;
    let asset_url = release
        .assets
        .into_iter()
        .find(|a| a.name.eq_ignore_ascii_case(ASSET_NAME))
        .and_then(|a| a.browser_download_url);
    let url = asset_url
        .or(release.html_url)
        .filter(|u| is_openable_url(u))
        .ok_or(NO_DOWNLOAD)?;
    Ok(Release {
        tag: release.tag_name,
        url,
    })
}

/// Only web links are handed to the shell, never a file path or other
/// scheme from a server reply.
pub fn is_openable_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
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

fn endpoint() -> Result<Url, String> {
    match std::env::var(URL_OVERRIDE_VAR) {
        Ok(value) if !value.trim().is_empty() => parse_url(value.trim(), true)
            .ok_or_else(|| format!("{URL_OVERRIDE_VAR} is not a valid http or https URL")),
        _ => parse_url(DEFAULT_URL, false).ok_or_else(|| BAD_URL.to_string()),
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
pub fn check_now() {
    start_check(false);
}

#[cfg(windows)]
pub fn check_auto() {
    start_check(true);
}

#[cfg(windows)]
pub fn open_download() {
    if let State::Available { url, .. } = state() {
        open_url(&url);
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

        let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n"
            .encode_utf16()
            .collect();
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
            if body.len() > MAX_BODY_BYTES {
                return Err(TOO_LARGE.to_string());
            }
        }
        Ok((status as u16, String::from_utf8_lossy(&body).into_owned()))
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
            {"name": "wrecktangle.exe", "browser_download_url": "https://github.com/x/releases/download/v0.3.0/wrecktangle.exe"}
        ]
    }"#;

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
    fn release_json_prefers_the_exe_asset() {
        let release = parse_release(RELEASE_JSON).unwrap();
        assert_eq!(release.tag, "v0.3.0");
        assert_eq!(
            release.url,
            "https://github.com/x/releases/download/v0.3.0/wrecktangle.exe"
        );
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
                url: "https://github.com/x/releases/download/v0.3.0/wrecktangle.exe".to_string(),
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
            }),
            "Version 9.9.9 is available"
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
                url: "https://github.com/x/releases/download/v0.3.0/wrecktangle.exe".to_string(),
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
