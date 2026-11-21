//! Text renders of what the app shows, so behaviour can be checked without
//! photographing the screen.

use crate::model::ALL_CATEGORIES;
use crate::prefs::Prefs;
use crate::store::SensorStore;

/// One sysctl string node, read in two passes. The value is cut at the first
/// NUL inside the buffer the kernel reported instead of trusting that it
/// terminated the value exactly there.
fn sysctl_string(name: &str) -> Option<String> {
    let name = std::ffi::CString::new(name).ok()?;
    let mut size = 0usize;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || size == 0
    {
        return None;
    }
    let mut buffer = vec![0u8; size];
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr() as *mut std::ffi::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

pub fn host_summary() -> String {
    let cpu = match sysctl_string("machdep.cpu.brand_string") {
        Some(cpu) if !cpu.is_empty() => cpu,
        _ => return "unknown host".to_string(),
    };
    format!("{cpu} · macOS {}", platform_version())
}

fn platform_version() -> String {
    sysctl_string("kern.osproductversion").unwrap_or_else(|| "?".to_string())
}

pub fn sensor_dump(store: &mut SensorStore, samples: usize, interval_seconds: f64) -> String {
    let mut out = String::new();
    out.push_str("# Vitals sensor dump\n");
    out.push_str(&format!("# host: {}\n", host_summary()));
    out.push_str(&format!(
        "# SMC: {} · {} keys total, {} polled per refresh\n",
        if store.smc_available() {
            "AppleSMC opened unprivileged"
        } else {
            "unavailable"
        },
        store.total_keys,
        store.polled_keys
    ));

    // A negative or non-finite interval panics inside `Duration`, halfway
    // through a dump that then looks like a crash rather than a bad flag.
    let interval_seconds = if interval_seconds.is_finite() {
        interval_seconds.clamp(0.0, 3600.0)
    } else {
        0.0
    };
    for sample in 0..samples.max(1) {
        if sample > 0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(interval_seconds));
        }
        store.refresh();
        out.push_str(&format!("\n# sample {}\n", sample + 1));
        for category in ALL_CATEGORIES {
            let reading = store.get(category);
            if reading.average.has_data() {
                out.push_str(&format!(
                    "## {}  —  average {}  ·  {} sensor(s)\n",
                    category.title(),
                    reading.describe(None),
                    reading.sensors.len()
                ));
            } else {
                out.push_str(&format!("## {}  —  No data\n", category.title()));
            }
            for sensor in &reading.sensors {
                let left = if sensor.key.is_empty() {
                    sensor.label.clone()
                } else {
                    format!("{}  {}", sensor.key, sensor.label)
                };
                out.push_str(&format!("   {:<32}{}\n", left, sensor.reading.text));
            }
        }
    }
    out
}

pub fn config_dump(prefs: &Prefs) -> String {
    // The menu bar renders the canonical order, not the insertion order.
    let slots: Vec<&str> = prefs.ordered_menu_bar().iter().map(|c| c.key()).collect();
    let mut selections: Vec<String> = Vec::new();
    for category in ALL_CATEGORIES {
        let choice = prefs
            .selection(category)
            .cloned()
            .unwrap_or_else(|| "average".to_string());
        selections.push(format!("{}={}", category.key(), choice));
    }
    format!(
        "menuBarCategories = {:?}\nrefreshInterval   = {} s\nstartAtLogin      = {}\nselections        = {}\n",
        slots,
        prefs.interval,
        prefs.start_at_login,
        selections.join(", ")
    )
}

pub fn selection_summary(prefs: &Prefs) -> String {
    let chosen: Vec<String> = ALL_CATEGORIES
        .iter()
        .filter_map(|category| {
            prefs
                .selection(*category)
                .map(|sensor| format!("{}={}", category.title(), sensor))
        })
        .collect();
    if chosen.is_empty() {
        "all Average".to_string()
    } else {
        chosen.join(", ")
    }
}
