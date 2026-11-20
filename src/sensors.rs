//! Builds every category: SMC-derived ones and the kernel-counter ones.

use std::ffi::{c_char, c_void, CStr};
use std::time::Instant;

use crate::model::{format, Category, CategoryReading, Reading, Sensor};

// MARK: - SMC-backed categories

pub fn temperature(values: &[(String, f64)]) -> CategoryReading {
    let mut sensors: Vec<Sensor> = values
        .iter()
        .filter(|(key, value)| {
            key.starts_with('T') && value.is_finite() && *value > 1.0 && *value < 130.0
        })
        .map(|(key, value)| Sensor {
            id: key.clone(),
            key: key.clone(),
            label: thermal_label(key).to_string(),
            reading: format::temperature(*value),
        })
        .collect();
    sensors.sort_by(|a, b| a.key.cmp(&b.key));
    if sensors.is_empty() {
        return CategoryReading::empty(Category::Temperature);
    }
    let mean = sensors.iter().map(|s| s.reading.value).sum::<f64>() / sensors.len() as f64;
    CategoryReading {
        category: Category::Temperature,
        sensors,
        average: format::temperature(mean),
    }
}

pub fn voltage(values: &[(String, f64)]) -> CategoryReading {
    let mut sensors: Vec<Sensor> = values
        .iter()
        .filter(|(key, value)| {
            key.starts_with('V') && value.is_finite() && *value > 0.05 && *value < 60.0
        })
        .map(|(key, value)| Sensor {
            id: key.clone(),
            key: key.clone(),
            label: voltage_label(key).to_string(),
            reading: format::voltage(*value),
        })
        .collect();
    sensors.sort_by(|a, b| a.key.cmp(&b.key));
    if sensors.is_empty() {
        return CategoryReading::empty(Category::Voltage);
    }
    let mean = sensors.iter().map(|s| s.reading.value).sum::<f64>() / sensors.len() as f64;
    CategoryReading {
        category: Category::Voltage,
        sensors,
        average: format::voltage(mean),
    }
}

pub fn fan(values: &[(String, f64)]) -> CategoryReading {
    let mut sensors: Vec<Sensor> = values
        .iter()
        .filter(|(key, value)| {
            let b = key.as_bytes();
            b.len() == 4
                && b[0] == b'F'
                && b[1].is_ascii_digit()
                && b[2] == b'A'
                && b[3] == b'c'
                && value.is_finite()
                && *value >= 0.0
                && *value < 12000.0
        })
        .map(|(key, value)| Sensor {
            id: key.clone(),
            key: key.clone(),
            label: format!("Fan {}", &key[1..2]),
            reading: format::rpm(*value),
        })
        .collect();
    sensors.sort_by(|a, b| a.key.cmp(&b.key));
    if sensors.is_empty() {
        return CategoryReading::empty(Category::Fan);
    }
    let mean = sensors.iter().map(|s| s.reading.value).sum::<f64>() / sensors.len() as f64;
    CategoryReading {
        category: Category::Fan,
        sensors,
        average: format::rpm(mean),
    }
}

fn thermal_label(key: &str) -> &'static str {
    match key {
        "TB0T" | "TB1T" | "TB2T" => return "Battery",
        "TC0P" | "TC0E" | "TC0D" => return "CPU Proximity",
        "TCMb" | "TCMz" => return "SoC",
        "TW0P" => return "Wi-Fi",
        "TPMP" => return "Power Management",
        "TS0P" | "TS1P" => return "SSD",
        _ => {}
    }
    let b = key.as_bytes();
    if b.len() != 4 || b[0] != b'T' {
        return "Thermal sensor";
    }
    match b[1] {
        b'C' => match b[2] {
            b'1' => "CPU E-Core",
            b'2' | b'3' => "CPU P-Core",
            _ => "CPU Core",
        },
        b'P' => {
            if b[2] == b'D' {
                "GPU Core"
            } else {
                "CPU Core"
            }
        }
        b'p' => "CPU P-Core",
        b'd' => "SoC Cluster",
        b'e' | b'g' | b'G' => "GPU Cluster",
        b'h' | b'm' | b'M' => "Memory",
        b'a' => "Ambient",
        b's' => "Sensor",
        b'v' | b'V' => "Media Engine",
        b'z' => "Unused",
        _ => "Thermal sensor",
    }
}

fn voltage_label(key: &str) -> &'static str {
    match key {
        "Vb0f" | "Vb1f" => return "DC-In",
        "VDMA" | "VDMM" => return "DRAM",
        "VMVC" => return "Memory Controller",
        "VSVR" => return "SoC Rail",
        _ => {}
    }
    let b = key.as_bytes();
    if b.len() != 4 {
        return "Voltage rail";
    }
    match b[1] {
        b'P' => "Power Rail",
        b'R' => "SoC Rail",
        b'C' => "CPU Rail",
        b'D' => "Voltage Rail",
        b'S' => "Sensor Rail",
        _ => "Voltage rail",
    }
}

// MARK: - Kernel counters

const HOST_VM_INFO64: i32 = 4;
const PROCESSOR_CPU_LOAD_INFO: i32 = 2;
const CPU_STATE_MAX: usize = 4;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct VmStatistics64 {
    free_count: u32,
    active_count: u32,
    inactive_count: u32,
    wire_count: u32,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: u64,
    speculative_count: u64,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: u32,
    throttled_count: u32,
    external_page_count: u32,
    internal_page_count: u32,
    total_uncompressed_pages_in_compressor: u64,
}

/// The leading fields of `struct if_data`, which is what `ifa_data` points at.
#[repr(C)]
struct IfDataHead {
    flags: [u8; 8],
    mtu: u32,
    metric: u32,
    baudrate: u32,
    ipackets: u32,
    ierrors: u32,
    opackets: u32,
    oerrors: u32,
    collisions: u32,
    ibytes: u32,
    obytes: u32,
}

extern "C" {
    fn host_statistics64(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
    fn host_processor_info(
        host: u32,
        flavor: i32,
        processor_count: *mut u32,
        info: *mut *mut i32,
        info_count: *mut u32,
    ) -> i32;
    fn vm_deallocate(target: u32, address: usize, size: usize) -> i32;
    fn mach_host_self() -> u32;
    static mach_task_self_: u32;

    fn IOPSCopyPowerSourcesInfo() -> *mut c_void;
    fn IOPSCopyPowerSourcesList(blob: *mut c_void) -> *mut c_void;
    fn IOPSGetPowerSourceDescription(blob: *mut c_void, source: *mut c_void) -> *mut c_void;
    fn CFArrayGetCount(array: *mut c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *mut c_void, index: isize) -> *mut c_void;
    fn CFRelease(cf: *mut c_void);
}

pub struct SystemSensors {
    previous_cpu: Vec<[u32; CPU_STATE_MAX]>,
    previous_network: Option<(u64, u64, Instant)>,
    page_size: u64,
}

impl SystemSensors {
    pub fn new() -> SystemSensors {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
        SystemSensors {
            previous_cpu: Vec::new(),
            previous_network: None,
            page_size,
        }
    }

    /// Seeds the delta-based samplers so the first refresh shows a real value.
    pub fn prime(&mut self) {
        let _ = self.processor();
        let _ = self.network();
    }

    pub fn memory(&self) -> CategoryReading {
        let total = match sysctl_u64("hw.memsize") {
            Some(value) if value > 0 => value,
            _ => return CategoryReading::empty(Category::Memory),
        };
        let mut stats = VmStatistics64::default();
        let mut count = (std::mem::size_of::<VmStatistics64>() / std::mem::size_of::<i32>()) as u32;
        let result = unsafe {
            host_statistics64(
                mach_host_self(),
                HOST_VM_INFO64,
                &mut stats as *mut VmStatistics64 as *mut i32,
                &mut count,
            )
        };
        if result != 0 {
            return CategoryReading::empty(Category::Memory);
        }

        let page = self.page_size;
        let active = stats.active_count as u64 * page;
        let wired = stats.wire_count as u64 * page;
        let compressed = stats.compressor_page_count as u64 * page;
        let free = stats.free_count as u64 * page;
        let inactive = stats.inactive_count as u64 * page;
        let used = active + wired + compressed;
        let used_percent = used as f64 / total as f64 * 100.0;

        let sensors = vec![
            Sensor {
                id: "memory.used.percent".into(),
                key: "used".into(),
                label: "Used".into(),
                reading: format::percent(used_percent),
            },
            Sensor {
                id: "memory.used.gb".into(),
                key: "used".into(),
                label: "Used (GB)".into(),
                reading: format::gigabytes(format::gigabytes_value(used)),
            },
            Sensor {
                id: "memory.app".into(),
                key: "active".into(),
                label: "App".into(),
                reading: format::gigabytes(format::gigabytes_value(active)),
            },
            Sensor {
                id: "memory.wired".into(),
                key: "wired".into(),
                label: "Wired".into(),
                reading: format::gigabytes(format::gigabytes_value(wired)),
            },
            Sensor {
                id: "memory.compressed".into(),
                key: "compressed".into(),
                label: "Compressed".into(),
                reading: format::gigabytes(format::gigabytes_value(compressed)),
            },
            Sensor {
                id: "memory.free".into(),
                key: "free".into(),
                label: "Free".into(),
                reading: format::gigabytes(format::gigabytes_value(free + inactive)),
            },
            Sensor {
                id: "memory.total".into(),
                key: "total".into(),
                label: "Total".into(),
                reading: format::gigabytes(format::gigabytes_value(total)),
            },
        ];
        CategoryReading {
            category: Category::Memory,
            sensors,
            average: format::percent(used_percent),
        }
    }

    pub fn processor(&mut self) -> CategoryReading {
        let mut cpu_count: u32 = 0;
        let mut info: *mut i32 = std::ptr::null_mut();
        let mut info_count: u32 = 0;
        let result = unsafe {
            host_processor_info(
                mach_host_self(),
                PROCESSOR_CPU_LOAD_INFO,
                &mut cpu_count,
                &mut info,
                &mut info_count,
            )
        };
        if result != 0 || info.is_null() {
            return CategoryReading::empty(Category::Processor);
        }

        let mut current: Vec<[u32; CPU_STATE_MAX]> = Vec::with_capacity(cpu_count as usize);
        unsafe {
            for cpu in 0..cpu_count as usize {
                let base = cpu * CPU_STATE_MAX;
                let mut ticks = [0u32; CPU_STATE_MAX];
                for lane in 0..CPU_STATE_MAX {
                    ticks[lane] = *info.add(base + lane) as u32;
                }
                current.push(ticks);
            }
            vm_deallocate(
                mach_task_self_,
                info as usize,
                info_count as usize * std::mem::size_of::<i32>(),
            );
        }

        let mut per_core = vec![0.0f64; current.len()];
        if self.previous_cpu.len() == current.len() {
            for (index, now) in current.iter().enumerate() {
                let before = &self.previous_cpu[index];
                let mut busy = 0.0;
                let mut total = 0.0;
                for lane in 0..CPU_STATE_MAX {
                    let delta = now[lane].wrapping_sub(before[lane]) as f64;
                    total += delta;
                    if lane != 2 {
                        // lane 2 is idle
                        busy += delta;
                    }
                }
                per_core[index] = if total > 0.0 { busy / total * 100.0 } else { 0.0 };
            }
        }
        self.previous_cpu = current;

        let total_load = if per_core.is_empty() {
            0.0
        } else {
            per_core.iter().sum::<f64>() / per_core.len() as f64
        };
        let mut sensors = vec![Sensor {
            id: "cpu.total".into(),
            key: "total".into(),
            label: "Total".into(),
            reading: format::percent(total_load),
        }];
        for (index, value) in per_core.iter().enumerate() {
            sensors.push(Sensor {
                id: format!("cpu.core.{index}"),
                key: format!("cpu{index}"),
                label: format!("Core {index}"),
                reading: format::percent(*value),
            });
        }
        CategoryReading {
            category: Category::Processor,
            sensors,
            average: format::percent(total_load),
        }
    }

    pub fn system_load(&self) -> CategoryReading {
        let mut loads = [0.0f64; 3];
        let count = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
        if count <= 0 {
            return CategoryReading::empty(Category::System);
        }
        let sensors = vec![
            Sensor {
                id: "load.1".into(),
                key: "1m".into(),
                label: "1 minute".into(),
                reading: format::load(loads[0]),
            },
            Sensor {
                id: "load.5".into(),
                key: "5m".into(),
                label: "5 minutes".into(),
                reading: format::load(loads[1]),
            },
            Sensor {
                id: "load.15".into(),
                key: "15m".into(),
                label: "15 minutes".into(),
                reading: format::load(loads[2]),
            },
        ];
        CategoryReading {
            category: Category::System,
            sensors,
            average: format::load(loads[0]),
        }
    }

    pub fn network(&mut self) -> CategoryReading {
        let Some((received_bytes, sent_bytes)) = interface_counters() else {
            return CategoryReading::empty(Category::Network);
        };
        let now = Instant::now();
        let mut received_rate = 0.0;
        let mut sent_rate = 0.0;
        if let Some((prev_rx, prev_tx, prev_at)) = self.previous_network {
            let elapsed = now.duration_since(prev_at).as_secs_f64();
            if elapsed > 0.05 {
                received_rate = received_bytes.saturating_sub(prev_rx) as f64 / elapsed;
                sent_rate = sent_bytes.saturating_sub(prev_tx) as f64 / elapsed;
            }
        }
        self.previous_network = Some((received_bytes, sent_bytes, now));

        let sensors = vec![
            Sensor {
                id: "net.received".into(),
                key: "rx".into(),
                label: "Received".into(),
                reading: format::bytes_per_second(received_rate),
            },
            Sensor {
                id: "net.sent".into(),
                key: "tx".into(),
                label: "Sent".into(),
                reading: format::bytes_per_second(sent_rate),
            },
        ];
        let average = Reading::new(
            received_rate + sent_rate,
            format!(
                "↓{}/s  ↑{}/s",
                format::scaled_bytes(received_rate),
                format::scaled_bytes(sent_rate)
            ),
            format!(
                "↓{} ↑{}",
                format::scaled_bytes(received_rate),
                format::scaled_bytes(sent_rate)
            ),
        );
        CategoryReading {
            category: Category::Network,
            sensors,
            average,
        }
    }

    pub fn storage(&self) -> CategoryReading {
        let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
        let path = std::ffi::CString::new("/").unwrap();
        let rc = unsafe { libc::statfs(path.as_ptr(), &mut stats) };
        if rc != 0 {
            return CategoryReading::empty(Category::Storage);
        }
        let block_size = stats.f_bsize as u64;
        let total = stats.f_blocks as u64 * block_size;
        let available = stats.f_bavail as u64 * block_size;
        if total == 0 {
            return CategoryReading::empty(Category::Storage);
        }
        let used = total.saturating_sub(available);
        let used_percent = used as f64 / total as f64 * 100.0;

        let sensors = vec![
            Sensor {
                id: "storage.available".into(),
                key: "available".into(),
                label: "Available".into(),
                reading: format::gigabytes(format::gigabytes_value(available)),
            },
            Sensor {
                id: "storage.used".into(),
                key: "used".into(),
                label: "Used".into(),
                reading: format::gigabytes(format::gigabytes_value(used)),
            },
            Sensor {
                id: "storage.total".into(),
                key: "total".into(),
                label: "Total".into(),
                reading: format::gigabytes(format::gigabytes_value(total)),
            },
        ];
        CategoryReading {
            category: Category::Storage,
            sensors,
            average: format::percent(used_percent),
        }
    }

    pub fn battery(&self, smc: &[(String, f64)]) -> CategoryReading {
        let lookup = |key: &str| smc.iter().find(|(k, _)| k == key).map(|(_, v)| *v);

        unsafe {
            let blob = IOPSCopyPowerSourcesInfo();
            if blob.is_null() {
                return CategoryReading::empty(Category::Battery);
            }
            let list = IOPSCopyPowerSourcesList(blob);
            if list.is_null() {
                CFRelease(blob);
                return CategoryReading::empty(Category::Battery);
            }
            let count = CFArrayGetCount(list);
            if count == 0 {
                CFRelease(list);
                CFRelease(blob);
                return CategoryReading::empty(Category::Battery);
            }
            let source = CFArrayGetValueAtIndex(list, 0);
            let description = IOPSGetPowerSourceDescription(blob, source);
            if description.is_null() {
                CFRelease(list);
                CFRelease(blob);
                return CategoryReading::empty(Category::Battery);
            }

            let current_capacity = dict_int(description, "Current Capacity").unwrap_or(0);
            let max_capacity = dict_int(description, "Max Capacity").unwrap_or(100);
            let charging = dict_bool(description, "Is Charging").unwrap_or(false);
            let state = dict_string(description, "Power Source State").unwrap_or_default();
            let on_ac = state == "AC Power";
            let charge_percent = if max_capacity > 0 {
                current_capacity as f64 / max_capacity as f64 * 100.0
            } else {
                current_capacity as f64
            };

            let mut sensors = vec![Sensor {
                id: "battery.charge".into(),
                key: "charge".into(),
                label: "Charge".into(),
                reading: format::percent(charge_percent),
            }];

            let voltage = lookup("B0AV").map(|mv| mv / 1000.0);
            let amperage = lookup("B0AC")
                .map(|ma| ma / 1000.0)
                .or_else(|| dict_int(description, "Current").map(|amps| amps as f64));
            if let (Some(volts), Some(amps)) = (voltage, amperage) {
                sensors.push(Sensor {
                    id: "battery.power".into(),
                    key: "B0AV×B0AC".into(),
                    label: "Power".into(),
                    reading: format::watts(volts * amps),
                });
                sensors.push(Sensor {
                    id: "battery.voltage".into(),
                    key: "B0AV".into(),
                    label: "Voltage".into(),
                    reading: format::voltage(volts),
                });
                sensors.push(Sensor {
                    id: "battery.current".into(),
                    key: "B0AC".into(),
                    label: "Current".into(),
                    reading: format::amperes(amps),
                });
            }

            let state_text = if charging {
                "Charging"
            } else if on_ac {
                "AC Power"
            } else {
                "On Battery"
            };
            sensors.push(Sensor {
                id: "battery.state".into(),
                key: "state".into(),
                label: "State".into(),
                reading: Reading::new(
                    if on_ac { 1.0 } else { 0.0 },
                    state_text.to_string(),
                    state_text.to_string(),
                ),
            });

            if !on_ac {
                if let Some(minutes) = dict_int(description, "Time to Empty") {
                    if minutes > 0 {
                        let text = format!("{}:{:02}", minutes / 60, minutes % 60);
                        sensors.push(Sensor {
                            id: "battery.remaining".into(),
                            key: "time".into(),
                            label: "Remaining".into(),
                            reading: Reading::new(minutes as f64, text.clone(), text),
                        });
                    }
                }
            }

            CFRelease(list);
            CFRelease(blob);

            CategoryReading {
                category: Category::Battery,
                sensors,
                average: format::percent(charge_percent),
            }
        }
    }
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let c_name = std::ffi::CString::new(name).ok()?;
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let rc = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            &mut value as *mut u64 as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 {
        Some(value)
    } else {
        None
    }
}

fn interface_counters() -> Option<(u64, u64)> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return None;
    }
    let mut received: u64 = 0;
    let mut sent: u64 = 0;
    let mut cursor = head;
    while !cursor.is_null() {
        let entry = unsafe { &*cursor };
        let address = entry.ifa_addr;
        if !address.is_null() && unsafe { (*address).sa_family as i32 } == libc::AF_LINK
            && !entry.ifa_data.is_null()
        {
            let name = unsafe { CStr::from_ptr(entry.ifa_name) }.to_string_lossy().into_owned();
            if !name.starts_with("lo") {
                let data = unsafe { &*(entry.ifa_data as *const IfDataHead) };
                received = received.wrapping_add(data.ibytes as u64);
                sent = sent.wrapping_add(data.obytes as u64);
            }
        }
        cursor = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(head) };
    Some((received, sent))
}

unsafe fn dict_value(dict: *mut c_void, key: &str) -> *mut c_void {
    // The IOPowerSources description is a CFDictionary; NSString keys are
    // toll-free bridged, so the objc runtime can build them.
    let key_obj = crate::objc::nsstring(key);
    let value = crate::objc::send_obj(dict as crate::objc::Obj, "objectForKey:", key_obj);
    crate::objc::release(key_obj);
    value
}

unsafe fn dict_int(dict: *mut c_void, key: &str) -> Option<isize> {
    let value = dict_value(dict, key);
    if value.is_null() {
        return None;
    }
    Some(crate::objc::send_int_result(value as crate::objc::Obj, "integerValue"))
}

unsafe fn dict_bool(dict: *mut c_void, key: &str) -> Option<bool> {
    let value = dict_value(dict, key);
    if value.is_null() {
        return None;
    }
    Some(crate::objc::send_bool_result(value as crate::objc::Obj, "boolValue"))
}

unsafe fn dict_string(dict: *mut c_void, key: &str) -> Option<String> {
    let value = dict_value(dict, key);
    if value.is_null() {
        return None;
    }
    let utf8 = crate::objc::send(value as crate::objc::Obj, "UTF8String") as *const c_char;
    if utf8.is_null() {
        return None;
    }
    Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
}
