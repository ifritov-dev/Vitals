//! Launching the app at login.
//!
//! SMAppService is a Swift-only API in this macOS, with no Objective-C class to
//! message, so the login item is an ordinary LaunchAgent: a plist in
//! ~/Library/LaunchAgents that launchd loads at login. Removing the file and
//! booting the job out turns it off again.

use std::path::PathBuf;
use std::process::Command;

const LABEL: &str = "com.daniel.vitals";

fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist")),
    )
}

pub fn enabled() -> bool {
    plist_path().map(|path| path.exists()).unwrap_or(false)
}

pub fn set(enabled: bool) -> Result<(), String> {
    let Some(path) = plist_path() else {
        return Err("HOME is not set".to_string());
    };
    if enabled {
        let Some(parent) = path.parent() else {
            return Err("no LaunchAgents directory".to_string());
        };
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        std::fs::write(&path, plist()).map_err(|error| error.to_string())?;
        // Re-registering an already loaded job fails, which is not an error
        // for us: the job is there either way.
        let _ = launchctl(&["bootstrap", &domain(), &path.to_string_lossy()]);
        return Ok(());
    }
    let _ = launchctl(&["bootout", &domain(), &path.to_string_lossy()]);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn domain() -> String {
    format!("gui/{}", unsafe { getuid() })
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let output = Command::new("/bin/launchctl")
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

extern "C" {
    fn getuid() -> u32;
}

fn plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Applications/Vitals.app/Contents/MacOS/Vitals</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#
    )
}

/// Takes the single-instance lock. A login item, a Finder launch and a
/// terminal run all start the binary, and only one of them gets to live.
pub fn claim_single_instance() -> bool {
    use std::os::unix::io::AsRawFd;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open("/tmp/com.daniel.vitals.lock")
    else {
        return true;
    };
    let taken = unsafe { flock(file.as_raw_fd(), 0x0002 | 0x0004 /* LOCK_EX | LOCK_NB */) } == 0;
    // The lock lives as long as the descriptor, which means the process.
    std::mem::forget(file);
    taken
}

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}
