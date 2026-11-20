//! Text renders of what the app shows, so behaviour can be checked without
//! photographing the screen.

use std::ffi::CStr;

use crate::model::{Category, ALL_CATEGORIES};
use crate::prefs::Prefs;
use crate::store::SensorStore;

extern "C" {
    fn sysctlbyname(
        name: *const std::ffi::c_char,
        oldp: *mut std::ffi::c_void,
        oldlenp: *mut usize,
        newp: *mut std::ffi::c_void,
        newlen: usize,
    ) -> i32;
}

pub fn host_summary() -> String {
    let mut size = 0usize;
    let name = std::ffi::CString::new("machdep.cpu.brand_string").unwrap();
    unsafe {
        if sysctlbyname(name.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) != 0
            || size == 0
        {
            return "unknown host".to_string();
        }
        let mut buffer = vec![0i8; size];
        if sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr() as *mut std::ffi::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return "unknown host".to_string();
        }
        let cpu = CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned();
        format!("{cpu} · macOS {}", platform_version())
    }
}

fn platform_version() -> String {
    let name = std::ffi::CString::new("kern.osproductversion").unwrap();
    let mut size = 0usize;
    unsafe {
        if sysctlbyname(name.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) != 0
            || size == 0
        {
            return "?".to_string();
        }
        let mut buffer = vec![0i8; size];
        if sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr() as *mut std::ffi::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return "?".to_string();
        }
        CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned()
    }
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
                    reading.average.text,
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
    let slots: Vec<&str> = prefs.menu_bar.iter().map(|c| c.key()).collect();
    let mut selections: Vec<String> = Vec::new();
    for category in ALL_CATEGORIES {
        let choice = prefs
            .selection(category)
            .cloned()
            .unwrap_or_else(|| "average".to_string());
        selections.push(format!("{}={}", category.key(), choice));
    }
    format!(
        "menuBarCategories = {:?}\nrefreshInterval   = {} s\nshowIcons         = {}\ncompactSlot       = {}\nselections        = {}\n",
        slots,
        prefs.interval,
        prefs.show_icons,
        prefs.compact,
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

pub fn category_letter(category: Category) -> &'static str {
    category.title()
}
