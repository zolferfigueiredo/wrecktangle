//! Launch at startup. Registered as a per-user Task Scheduler logon task,
//! which starts as soon as the user signs in. Explorer starts Run-key apps
//! only after the desktop is up, after a delay and one at a time, so a Run
//! value can leave Wrecktangle waiting behind every other startup app. The
//! Run value is kept as the fallback when the task can't be created.

#[cfg(windows)]
use windows::Win32::Foundation::{E_FAIL, ERROR_FILE_NOT_FOUND, WIN32_ERROR};
#[cfg(windows)]
use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
#[cfg(windows)]
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SAM_FLAGS, REG_SZ, RegCloseKey,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
#[cfg(windows)]
use windows::Win32::System::TaskScheduler::{
    ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN, TaskScheduler,
};
#[cfg(windows)]
use windows::Win32::System::Variant::VARIANT;
#[cfg(windows)]
use windows::core::{BSTR, Error, HRESULT, PCWSTR, PWSTR, w};

#[cfg(windows)]
const RUN_KEY_PATH: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
#[cfg(windows)]
const VALUE_NAME: PCWSTR = w!("Wrecktangle");
// Written by the app under its earlier name, Wectangle.
#[cfg(windows)]
const LEGACY_VALUE_NAME: PCWSTR = w!("Wectangle");

/// The logon task's name. Task names are shared by every account on the
/// machine, so it carries the user name. installer/wrecktangle.iss deletes
/// the task by this name on uninstall, with Inno's `{username}`.
pub fn task_name(user: &str) -> String {
    format!("Wrecktangle ({user})")
}

/// The user name in a `DOMAIN\user` account, as GetUserNameW returns it.
pub fn account_user(account: &str) -> &str {
    account.rsplit('\\').next().unwrap_or(account)
}

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// The logon task for `account` (`DOMAIN\user`) that runs `command`.
/// The trigger names the account because a standard user may only create
/// logon tasks for itself. Priority 4 is the normal priority a Run-key launch
/// gets; Task Scheduler's default, 7, is below normal with low I/O priority.
/// The default 72-hour time limit would end the app, so there is none.
pub fn task_xml(account: &str, command: &str) -> String {
    let account = xml_escape(account);
    let command = xml_escape(command);
    format!(
        r#"<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Wrecktangle</Author>
    <Description>Starts Wrecktangle when you sign in.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{account}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{account}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// Whether a Run value launches the same exe as `command`. Paths on Windows
/// are case-insensitive.
pub fn same_command(value: &str, command: &str) -> bool {
    value.trim().to_lowercase() == command.trim().to_lowercase()
}

#[cfg(windows)]
fn check(err: WIN32_ERROR) -> windows::core::Result<()> {
    if err.0 == 0 {
        Ok(())
    } else {
        Err(Error::from_hresult(HRESULT::from_win32(err.0)))
    }
}

#[cfg(windows)]
fn current_exe_quoted() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

#[cfg(windows)]
fn open_run_key(access: REG_SAM_FLAGS) -> windows::core::Result<HKEY> {
    let mut hkey = HKEY::default();
    unsafe {
        check(RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY_PATH,
            Some(0),
            access,
            &mut hkey,
        ))?;
    }
    Ok(hkey)
}

#[cfg(windows)]
fn run_value() -> Option<String> {
    let hkey = open_run_key(KEY_READ).ok()?;
    let mut buf = [0u16; 2048];
    let mut len = (buf.len() * 2) as u32;
    let found = unsafe {
        let found = check(RegQueryValueExW(
            hkey,
            VALUE_NAME,
            None,
            None,
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut len),
        ))
        .is_ok();
        let _ = RegCloseKey(hkey);
        found
    };
    if !found {
        return None;
    }
    let value = String::from_utf16_lossy(&buf[..len as usize / 2]);
    Some(value.trim_end_matches('\0').to_string())
}

#[cfg(windows)]
fn write_run_value(command: &str) -> windows::core::Result<()> {
    let hkey = open_run_key(KEY_WRITE)?;
    let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let result = check(RegSetValueExW(
            hkey,
            VALUE_NAME,
            Some(0),
            REG_SZ,
            Some(bytes),
        ));
        let _ = RegCloseKey(hkey);
        result
    }
}

#[cfg(windows)]
fn delete_run_value() -> windows::core::Result<()> {
    let hkey = open_run_key(KEY_WRITE)?;
    unsafe {
        let err = RegDeleteValueW(hkey, VALUE_NAME);
        let _ = RegCloseKey(hkey);
        if err == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(err)
        }
    }
}

/// The signed-in account as `DOMAIN\user`.
#[cfg(windows)]
fn account() -> windows::core::Result<String> {
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    unsafe {
        if !GetUserNameExW(NameSamCompatible, Some(PWSTR(buf.as_mut_ptr())), &mut len) {
            return Err(Error::from_thread());
        }
    }
    Ok(String::from_utf16_lossy(&buf[..len as usize]))
}

#[cfg(windows)]
fn task_folder() -> windows::core::Result<ITaskFolder> {
    unsafe {
        let service: ITaskService = CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        let empty = VARIANT::default();
        service.Connect(&empty, &empty, &empty, &empty)?;
        service.GetFolder(&BSTR::from("\\"))
    }
}

#[cfg(windows)]
fn register_task(command: &str) -> windows::core::Result<()> {
    let account = account()?;
    let empty = VARIANT::default();
    unsafe {
        task_folder()?.RegisterTask(
            &BSTR::from(task_name(account_user(&account))),
            &BSTR::from(task_xml(&account, command)),
            TASK_CREATE_OR_UPDATE.0,
            &empty,
            &empty,
            TASK_LOGON_INTERACTIVE_TOKEN,
            &empty,
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn delete_task() -> windows::core::Result<()> {
    let name = task_name(account_user(&account()?));
    match unsafe { task_folder()?.DeleteTask(&BSTR::from(name), 0) } {
        Err(e) if e.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => Ok(()),
        result => result,
    }
}

#[cfg(windows)]
fn task_enabled() -> bool {
    let enabled = || -> windows::core::Result<bool> {
        let name = task_name(account_user(&account()?));
        unsafe {
            let task = task_folder()?.GetTask(&BSTR::from(name))?;
            Ok(task.Enabled()?.as_bool())
        }
    };
    enabled().unwrap_or(false)
}

#[cfg(windows)]
pub fn is_enabled() -> bool {
    task_enabled() || run_value().is_some()
}

/// Turns launch at startup on with the logon task, or with the Run value if
/// the task can't be created, or turns both off.
#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> windows::core::Result<()> {
    if !enabled {
        let task = delete_task();
        delete_run_value()?;
        return task;
    }
    let command = current_exe_quoted().ok_or_else(|| Error::from_hresult(E_FAIL))?;
    if register_task(&command).is_ok() {
        delete_run_value()
    } else {
        write_run_value(&command)
    }
}

/// Replaces a launch-at-startup entry left under the old name with one for
/// this exe.
#[cfg(windows)]
pub fn migrate_legacy() {
    let Ok(hkey) = open_run_key(KEY_WRITE) else {
        return;
    };
    let had_legacy = unsafe {
        let had_legacy = RegDeleteValueW(hkey, LEGACY_VALUE_NAME).0 == 0;
        let _ = RegCloseKey(hkey);
        had_legacy
    };
    if had_legacy {
        let _ = set_enabled(true);
    }
}

/// Moves a Run value for this exe, left by an earlier version or written by
/// the installer's "Launch at startup" checkbox, to the logon task. A Run
/// value for another copy (an installed one, when this is a dev build) is
/// left alone.
#[cfg(windows)]
pub fn migrate_run_value() {
    let (Some(value), Some(command)) = (run_value(), current_exe_quoted()) else {
        return;
    };
    if same_command(&value, &command) && register_task(&command).is_ok() {
        let _ = delete_run_value();
    }
}

#[cfg(windows)]
pub fn toggle() -> windows::core::Result<bool> {
    let enabled = !is_enabled();
    set_enabled(enabled)?;
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_name_carries_the_user() {
        assert_eq!(task_name("zolfe"), "Wrecktangle (zolfe)");
    }

    #[test]
    fn account_user_drops_the_domain() {
        assert_eq!(account_user("DESKTOP-1\\zolfe"), "zolfe");
        assert_eq!(account_user("zolfe"), "zolfe");
    }

    #[test]
    fn task_xml_escapes_the_account_and_command() {
        let xml = task_xml("PC\\A&B", "\"C:\\Users\\A&B\\<x>\\wrecktangle.exe\"");
        assert!(xml.contains("<UserId>PC\\A&amp;B</UserId>"));
        assert!(xml.contains(
            "<Command>&quot;C:\\Users\\A&amp;B\\&lt;x&gt;\\wrecktangle.exe&quot;</Command>"
        ));
        assert!(!xml.contains("A&B"));
    }

    #[test]
    fn task_xml_starts_at_this_users_logon_and_keeps_running() {
        let xml = task_xml("PC\\zolfe", "\"C:\\wrecktangle.exe\"");
        let trigger = xml
            .split("<LogonTrigger>")
            .nth(1)
            .and_then(|rest| rest.split("</LogonTrigger>").next())
            .unwrap();
        assert!(trigger.contains("<UserId>PC\\zolfe</UserId>"));
        assert!(!trigger.contains("<Delay>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains("<Priority>4</Priority>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
    }

    #[test]
    fn same_command_ignores_case_and_spacing() {
        let command =
            "\"C:\\Users\\Zolfe\\AppData\\Local\\Programs\\Wrecktangle\\wrecktangle.exe\"";
        assert!(same_command(
            "\"c:\\users\\zolfe\\appdata\\local\\programs\\wrecktangle\\wrecktangle.exe\" ",
            command
        ));
        assert!(!same_command(
            "\"C:\\dev\\wrecktangle\\target\\release\\wrecktangle.exe\"",
            command
        ));
    }
}
