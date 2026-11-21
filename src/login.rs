//! Launching the app at login.
//!
//! SMAppService is a Swift-only API in this macOS, with no Objective-C class to
//! message, so the login item is an ordinary LaunchAgent: a plist in
//! ~/Library/LaunchAgents that launchd loads at login.
//!
//! macOS deletes that plist by itself. backgroundtaskmanagementd prunes an
//! agent whose registration it has seen too often — "Exceeded max
//! notifications for Vitals" — and the file goes with it, so the file cannot
//! be what the setting remembers. The flag lives in Prefs, and `apply`
//! rebuilds the agent from it whenever the app starts.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::objc;

const LABEL: &str = "com.ifritov.vitals";
/// Labels this app used before the rename; their agents keep launching the
/// app at login and only this sweep can turn them off.
const LEGACY_LABELS: [&str; 1] = ["com.daniel.vitals"];
const INSTALLED: &str = "/Applications/Vitals.app/Contents/MacOS/Vitals";

fn plist_path_for(label: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist")),
    )
}

fn plist_path() -> Option<PathBuf> {
    plist_path_for(LABEL)
}

pub fn installed() -> bool {
    plist_path().map(|path| path.exists()).unwrap_or(false)
}

pub fn loaded() -> bool {
    launchctl(&["print", &service(LABEL)]).is_ok()
}

fn service(label: &str) -> String {
    format!("{}/{}", domain(), label)
}

fn domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

/// Makes the login agent match `start_at_login`, which is what the setting
/// says, rather than reading the agent back to find out what it says.
///
/// The launchctl calls are synchronous: the whole path is one or two calls of
/// about ten milliseconds, and it is the only way a caller learns that the
/// registration failed instead of a checkmark that lies.
pub fn apply(start_at_login: bool) -> Result<(), String> {
    let Some(path) = plist_path() else {
        return Err("HOME is not set".to_string());
    };
    sweep_legacy_labels();
    if start_at_login {
        return enable(&path);
    }
    // A job launchd still holds from an earlier registration outlives the
    // file, and a registered job is enough to start the app at login, so it
    // has to be booted out even when the file is already gone.
    let _ = launchctl(&["bootout", &service(LABEL)]);
    remove_plist(&path)
}

fn sweep_legacy_labels() {
    for label in LEGACY_LABELS {
        let Some(path) = plist_path_for(label) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let _ = launchctl(&["bootout", &service(label)]);
        let _ = remove_plist(&path);
    }
}

fn remove_plist(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn enable(path: &Path) -> Result<(), String> {
    let program = program_path().ok_or_else(|| format!("no executable at {INSTALLED}"))?;
    let plist = plist(&program);
    if std::fs::read_to_string(path).ok().as_deref() != Some(plist.as_str()) {
        let parent = path.parent().ok_or("no LaunchAgents directory")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        std::fs::write(path, plist).map_err(|error| error.to_string())?;
        // Registering over a job launchd already holds fails with
        // "5: Input/output error" and leaves the old job running the old
        // binary, so drop it first; an absent job is the normal case.
        let _ = launchctl(&["bootout", &service(LABEL)]);
    }
    if loaded() {
        return Ok(());
    }
    launchctl(&["bootstrap", &domain(), &path.to_string_lossy()])
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

/// Whether this process runs from an .app bundle, and so shares the
/// preference domain the menu reads.
pub fn bundled() -> bool {
    running_bundle().is_some()
}

fn running_bundle() -> Option<String> {
    unsafe {
        let bundle = objc::send(objc::class("NSBundle"), "mainBundle");
        if bundle.is_null() {
            return None;
        }
        let path = objc::nsstring_to_string(objc::send(bundle, "bundlePath"));
        path.ends_with(".app").then_some(path)
    }
}

/// The bundle that is running, unless it is a throwaway copy under a build
/// directory: `make run` installs from `dist/`, and `make clean` or the next
/// `make app` deletes it, so the installed copy is the one that survives.
fn program_path() -> Option<String> {
    running_bundle()
        .map(|bundle| format!("{bundle}/Contents/MacOS/Vitals"))
        .filter(|program| Path::new(program).exists() && !program.contains("/dist/"))
        .or_else(|| {
            let installed = INSTALLED.to_string();
            Path::new(&installed).exists().then_some(installed)
        })
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn plist(program: &str) -> String {
    let program = xml_escape(program);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
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
/// terminal run all start the binary, and only one of them gets to live. The
/// file sits in the app's own support folder, so it is neither shared with
/// other users nor reaped when /tmp is cleaned.
pub fn claim_single_instance() -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return true;
    };
    let directory = PathBuf::from(home).join("Library/Application Support/Vitals");
    if std::fs::create_dir_all(&directory).is_err() {
        return true;
    }
    let path = directory.join("vitals.lock");
    let Ok(path) = std::ffi::CString::new(path.to_string_lossy().into_owned()) else {
        return true;
    };
    // O_NOFOLLOW: a lock file that turns into a symlink is not one to open.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_CREAT | libc::O_RDWR | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return true;
    }
    let taken = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0;
    // The lock lives as long as the descriptor, which means the process.
    if !taken {
        unsafe { libc::close(fd) };
    }
    taken
}
