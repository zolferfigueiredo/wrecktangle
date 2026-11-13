//! Lists, removes and restores Xfce's keyboard shortcuts with
//! `xfconf-query`, and keeps the record of what Wrecktangle removed. The
//! parsing is in `src/xfce.rs`.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use crate::config;
use crate::xfce::{self, Binding, Record};

const CHANNEL: &str = "xfce4-keyboard-shortcuts";

fn query(args: &[&str]) -> Option<Output> {
    Command::new("xfconf-query")
        .arg("-c")
        .arg(CHANNEL)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
}

/// Xfce's custom shortcuts, or None without Xfce.
pub fn list() -> Option<Vec<Binding>> {
    let output = query(&["-l", "-v"]).filter(|o| o.status.success())?;
    Some(xfce::parse_list(&String::from_utf8_lossy(&output.stdout)))
}

pub fn remove(binding: &Binding) -> bool {
    query(&["-p", &binding.property, "-r"]).is_some_and(|o| o.status.success())
}

pub fn restore(binding: &Binding) -> bool {
    query(&[
        "-p",
        &binding.property,
        "-n",
        "-t",
        "string",
        "-s",
        &binding.value,
    ])
    .is_some_and(|o| o.status.success())
}

fn record_path() -> PathBuf {
    config::app_config_dir().join("xfce-takeover.json")
}

pub fn load_record() -> Record {
    std::fs::read_to_string(record_path())
        .map(|text| Record::from_json(&text))
        .unwrap_or_default()
}

pub fn save_record(record: &Record) {
    let path = record_path();
    if record.removed.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, record.to_json());
}

/// Puts back every binding in the record. One that fails stays in it, so
/// the next start or the uninstall script can try again.
pub fn restore_all() {
    let mut record = load_record();
    record.removed.retain(|binding| !restore(binding));
    save_record(&record);
}
