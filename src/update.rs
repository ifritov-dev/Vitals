//! Updating from the releases published on GitHub.
//!
//! An app with no dependencies cannot link a TLS stack, so the network is the
//! system curl and the transport problems it already solves. curl is also what
//! makes a check safe to run from a menu: `--max-time` bounds it, and a dead
//! network ends in an error string instead of a hung process.
//!
//! The release JSON is read by a scanner rather than a parser. Three fields are
//! all the menu needs, and a scanner that only knows string tokens cannot be
//! fooled by a release body that happens to quote a key name.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::Command;

const CURL: &str = "/usr/bin/curl";
const HDITUTIL: &str = "/usr/bin/hdiutil";
const DITTO: &str = "/usr/bin/ditto";
const LATEST: &str = "https://api.github.com/repos/ifritov-dev/Vitals/releases/latest";
/// Where `make install` and the released DMG put the app. The update replaces
/// this copy and nothing else: a build inside `dist/` is not an installation.
const INSTALLED: &str = "/Applications/Vitals.app";

pub struct Update {
    pub version: String,
    pub dmg_url: String,
    pub notes: String,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Asks GitHub for the newest release. `Ok(None)` covers both "this build is
/// the newest" and "nothing has been published yet"; an error is only ever
/// something the menu can show as a real failure, such as a network that is
/// down or a rate limit.
pub fn check() -> Result<Option<Update>, String> {
    let agent = format!("User-Agent: Vitals/{}", current_version());
    let body = curl(&[
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        &agent,
        "--connect-timeout",
        "5",
        "--max-time",
        "20",
        "-f",
        "-sS",
        LATEST,
    ])?;
    let Some(body) = body else {
        return Ok(None);
    };
    let update = parse_release(&body)
        .ok_or_else(|| "could not read the release GitHub returned".to_string())?;
    Ok((compare(&update.version, current_version()) == Ordering::Greater).then_some(update))
}

/// Downloads the release image, mounts it and swaps the installed bundle for
/// the one inside. Blocking, and long enough that the menu has to call it off
/// the main thread.
#[allow(dead_code)]
pub fn install(update: &Update) -> Result<(), String> {
    let target = Path::new(INSTALLED);
    // Copying somewhere else would install a second app the user never opens,
    // or nothing at all, so say why instead of guessing.
    if !target.is_dir() {
        return Err(format!(
            "Vitals is not installed in {INSTALLED}, so there is no copy to replace"
        ));
    }

    let mut scratch = Scratch::new()?;
    let image = scratch.dir.join("Vitals.dmg");
    let image_path = image.to_string_lossy().into_owned();
    if curl(&[
        "-f",
        "-sS",
        "-L",
        "--connect-timeout",
        "10",
        "--max-time",
        "600",
        "-o",
        &image_path,
        &update.dmg_url,
    ])?
    .is_none()
    {
        return Err(format!("could not download {}: not found", update.dmg_url));
    }

    // A truncated download verifies as a corrupt image, which is a clearer
    // failure than an attach that reports an error nobody can read.
    run(HDITUTIL, &["verify", &image_path])
        .map_err(|error| format!("downloaded image is not usable: {error}"))?;
    let mount = scratch.attach(&image)?;
    let source = bundle_in(&mount)?;

    let staged = scratch.dir.join("Vitals.app");
    run(
        DITTO,
        &[&source.to_string_lossy(), &staged.to_string_lossy()],
    )
    .map_err(|error| format!("could not copy the new app out of the image: {error}"))?;
    // Detached before the swap: the volume holds a reference to the image, and
    // a failure from here on must not leave it mounted.
    scratch.detach();

    swap(&staged, target)?;

    // The download carries Gatekeeper's quarantine flag, and a bundle that
    // keeps it asks for a confirmation the user already gave by choosing to
    // update. Clearing it can fail when the download was not quarantined at
    // all, which is not a reason to strand the new copy.
    let _ = run(
        "/usr/bin/xattr",
        &["-dr", "com.apple.quarantine", INSTALLED],
    );
    run("/usr/bin/open", &["-n", INSTALLED])
        .map(|_| ())
        .map_err(|error| format!("update installed, but the new copy did not start: {error}"))
}

/// The temp directory and the volume an install mounts. Both have to go on
/// every path, including the ones that return an error halfway through.
struct Scratch {
    dir: PathBuf,
    mount: Option<PathBuf>,
}

impl Scratch {
    /// Named for the process and the moment, so an attempt that died before its
    /// cleanup cannot lend a half-downloaded image to the next one.
    fn new() -> Result<Self, String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("vitals-update-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("could not make {}: {error}", dir.display()))?;
        Ok(Self { dir, mount: None })
    }

    fn attach(&mut self, image: &Path) -> Result<PathBuf, String> {
        let output = run(
            HDITUTIL,
            &[
                "attach",
                "-nobrowse",
                "-readonly",
                &image.to_string_lossy(),
            ],
        )
        .map_err(|error| format!("could not mount {}: {error}", image.display()))?;
        // One line per device, with the mount point as its last column; the
        // volume lines are the ones that end in a /Volumes path.
        let mount = output
            .lines()
            .filter_map(|line| line.split('\t').next_back())
            .map(str::trim)
            .find(|column| column.starts_with("/Volumes/"))
            .map(PathBuf::from)
            .ok_or_else(|| format!("could not find where {} was mounted", image.display()))?;
        self.mount = Some(mount.clone());
        Ok(mount)
    }

    fn detach(&mut self) {
        if let Some(mount) = self.mount.take() {
            let _ = run(HDITUTIL, &["detach", &mount.to_string_lossy()]);
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        self.detach();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The app bundle in a mounted image. The released image is expected to hold
/// one named like the installed copy, and an image holding exactly one bundle
/// is unambiguous even when the name differs.
fn bundle_in(mount: &Path) -> Result<PathBuf, String> {
    let expected = mount.join("Vitals.app");
    if expected.is_dir() {
        return Ok(expected);
    }
    let found: Vec<PathBuf> = std::fs::read_dir(mount)
        .map_err(|error| format!("could not read {}: {error}", mount.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().map(|kind| kind == "app").unwrap_or(false))
        .collect();
    match found.as_slice() {
        [] => Err(format!(
            "no app bundle in the image mounted at {}",
            mount.display()
        )),
        [only] => Ok(only.clone()),
        several => Err(format!(
            "the image mounted at {} holds {} app bundles",
            mount.display(),
            several.len()
        )),
    }
}

/// Puts the staged bundle in place and puts the installed one back if the copy
/// fails. The backup sits beside the target because the temp directory can be
/// on another volume, where a rename would not be possible.
fn swap(staged: &Path, target: &Path) -> Result<(), String> {
    let backup = PathBuf::from(format!("{}.previous", target.display()));
    // A backup an interrupted run left behind holds a bundle that is either
    // installed already or gone; keeping it would fail the rename below.
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(target, &backup)
        .map_err(|error| format!("could not move the installed copy aside: {error}"))?;
    match run(DITTO, &[&staged.to_string_lossy(), &target.to_string_lossy()]) {
        Ok(_) => {
            let _ = std::fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(error) => {
            // Without this the user is left with no app at all, which is much
            // worse than the update having failed.
            let _ = std::fs::remove_dir_all(target);
            match std::fs::rename(&backup, target) {
                Ok(()) => Err(format!("could not install the update: {error}")),
                Err(restore) => Err(format!(
                    "could not install the update: {error}; the previous copy is at {} ({restore})",
                    backup.display()
                )),
            }
        }
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if message.is_empty() {
        format!("{program} failed")
    } else {
        message
    })
}

/// Runs curl and returns its body. `Ok(None)` is the server answering 404,
/// which is how a repository with no published release looks: a normal answer
/// rather than something to report. Anything else non-zero is an error carrying
/// curl's own words, since that is what the menu has to show.
fn curl(args: &[&str]) -> Result<Option<String>, String> {
    let output = Command::new(CURL)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {CURL}: {error}"))?;
    if output.status.success() {
        return Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()));
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    // The status is only ever in the message: curl exits 22 for an HTTP error
    // on most builds and 56 on this one, so the code says nothing reliable and
    // every caller passes `-sS`, which keeps the message.
    if stderr.contains("error: 404") {
        return Ok(None);
    }
    Err(if stderr.is_empty() {
        format!("curl exited with {}", output.status)
    } else {
        stderr
    })
}

/// The fields the menu needs out of one release.
pub(crate) fn parse_release(json: &str) -> Option<Update> {
    let (tag, _) = field(json, "tag_name", 0)?;
    let version = tag.trim_start_matches('v').trim_start_matches('V').to_string();
    let notes = field(json, "body", 0)
        .map(|(body, _)| body.trim_end().to_string())
        .unwrap_or_default();
    Some(Update {
        version,
        dmg_url: dmg_asset(json)?,
        notes,
    })
}

/// The download URL of the first asset that is a disk image. A release carries
/// a sources archive and a checksum beside the DMG, and only the DMG is
/// something the installer can mount.
fn dmg_asset(json: &str) -> Option<String> {
    let mut cursor = find_key(json, "assets", 0)?;
    while let Some((token, after)) = next_string(json, cursor) {
        if token != "name" {
            cursor = after;
            continue;
        }
        let (name, end) = value_after(json, after)?;
        if name.to_ascii_lowercase().ends_with(".dmg") {
            return field(json, "browser_download_url", end).map(|(url, _)| url);
        }
        cursor = end;
    }
    None
}

/// The decoded value of `key`, searched for from `from`, with the offset just
/// past it.
fn field(json: &str, key: &str, from: usize) -> Option<(String, usize)> {
    value_after(json, find_key(json, key, from)?)
}

/// The offset just past the first `"key"` token at or after `from`, whatever
/// kind of value follows it.
fn find_key(json: &str, key: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    while let Some((token, after)) = next_string(json, cursor) {
        if token == key {
            return Some(after);
        }
        cursor = after;
    }
    None
}

/// The string value that follows a key. A key whose value is not a string —
/// an object, an array, a number — yields None, so a name that matches by
/// accident cannot be mistaken for the field.
fn value_after(json: &str, from: usize) -> Option<(String, usize)> {
    let bytes = json.as_bytes();
    let mut index = from;
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    if *bytes.get(index)? != b':' {
        return None;
    }
    index += 1;
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    if *bytes.get(index)? != b'"' {
        return None;
    }
    next_string(json, index)
}

/// The next string token at or after `from`, decoded, with the offset just past
/// its closing quote. Walking whole tokens keeps the scan out of string
/// contents: a release body quoting `"tag_name"` is one token, never a key.
fn next_string(json: &str, from: usize) -> Option<(String, usize)> {
    let bytes = json.as_bytes();
    let mut index = from;
    while *bytes.get(index)? != b'"' {
        index += 1;
    }
    index += 1;
    let mut value = String::new();
    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'"' => return Some((value, index + 1)),
            b'\\' => {
                index += 1;
                match bytes.get(index).copied()? {
                    b'"' => value.push('"'),
                    b'\\' => value.push('\\'),
                    b'/' => value.push('/'),
                    b'b' => value.push('\u{8}'),
                    b'f' => value.push('\u{c}'),
                    b'n' => value.push('\n'),
                    b'r' => value.push('\r'),
                    b't' => value.push('\t'),
                    b'u' => {
                        let hex = json.get(index + 1..index + 5)?;
                        let code = u32::from_str_radix(hex, 16).ok()?;
                        // A lone surrogate is not a char; a release note is not
                        // worth failing an update over.
                        value.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        index += 4;
                    }
                    // An escape JSON does not define: keep the character as it
                    // was written rather than inventing a meaning for it.
                    other => value.push(other as char),
                }
                index += 1;
            }
            _ => {
                // Copy the run up to the next quote or escape in one go, so
                // multi-byte characters pass through untouched.
                let start = index;
                while index < bytes.len() && bytes[index] != b'"' && bytes[index] != b'\\' {
                    index += 1;
                }
                value.push_str(json.get(start..index)?);
            }
        }
    }
    None
}

/// Dotted numeric comparison, which is the only order GitHub tags follow here.
/// A leading `v` is decoration, and a `-rc1` suffix names the release the tag
/// leads to rather than a version below it.
pub(crate) fn compare(left: &str, right: &str) -> Ordering {
    let left = numbers(left);
    let right = numbers(right);
    for index in 0..left.len().max(right.len()) {
        let ordering = left
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right.get(index).copied().unwrap_or(0));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn numbers(version: &str) -> Vec<u64> {
    let version = version.trim();
    let version = version.trim_start_matches('v').trim_start_matches('V');
    let version = version.split('-').next().unwrap_or(version);
    // A component that is not a number counts as zero: a tag of another shape
    // must not turn into a panic in the middle of an update check.
    version
        .split('.')
        .map(|part| part.trim().parse::<u64>().unwrap_or(0))
        .collect()
}
