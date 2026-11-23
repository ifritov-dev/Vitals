//! Memory, processor, system, network and storage from mach and sysctl.

use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::time::Instant;

use crate::model::{format, CategoryReading, Reading, Sensor};

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
    // natural_t (u32) in XNU's vm_statistics64; u64 here shifts every later field.
    purgeable_count: u32,
    speculative_count: u32,
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

}

pub struct SystemSensors {
    previous_cpu: Vec<[u32; CPU_STATE_MAX]>,
    /// Per-interface counters, so one interface wrapping its 32-bit counter
    /// cannot make the summed rate read as zero.
    previous_network: Option<(HashMap<String, (u32, u32)>, Instant)>,
    page_size: u64,
    /// The host port is constant for the life of the process; taking a fresh
    /// send right per poll leaks one into the kernel every time.
    host: u32,
    /// Per-core id/label strings, built once instead of formatted per poll.
    core_descriptors: Vec<(String, String)>,
}

impl SystemSensors {
    pub fn new() -> SystemSensors {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
        SystemSensors {
            previous_cpu: Vec::new(),
            previous_network: None,
            page_size,
            host: unsafe { mach_host_self() },
            core_descriptors: Vec::new(),
        }
    }

    /// Seeds the delta-based samplers so the first refresh shows a real value.
    /// The two reads happen microseconds apart, which is too short a window
    /// for CPU and network deltas, so the prime waits one poll's worth of a
    /// window instead of publishing a 0 %-or-100 % artefact.
    pub fn prime(&mut self) {
        let _ = self.processor();
        let _ = self.network();
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    pub fn memory(&self) -> CategoryReading {
        let total = match sysctl_u64("hw.memsize") {
            Some(value) if value > 0 => value,
            _ => return CategoryReading::empty(),
        };
        let mut stats = VmStatistics64::default();
        let mut count = (std::mem::size_of::<VmStatistics64>() / std::mem::size_of::<i32>()) as u32;
        let result = unsafe {
            host_statistics64(
                self.host,
                HOST_VM_INFO64,
                &mut stats as *mut VmStatistics64 as *mut i32,
                &mut count,
            )
        };
        if result != 0 {
            return CategoryReading::empty();
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
                reading: format::gibibytes(format::gibibytes_value(used)),
            },
            Sensor {
                id: "memory.app".into(),
                key: "active".into(),
                label: "App".into(),
                reading: format::gibibytes(format::gibibytes_value(active)),
            },
            Sensor {
                id: "memory.wired".into(),
                key: "wired".into(),
                label: "Wired".into(),
                reading: format::gibibytes(format::gibibytes_value(wired)),
            },
            Sensor {
                id: "memory.compressed".into(),
                key: "compressed".into(),
                label: "Compressed".into(),
                reading: format::gibibytes(format::gibibytes_value(compressed)),
            },
            Sensor {
                id: "memory.free".into(),
                key: "free".into(),
                label: "Free".into(),
                reading: format::gibibytes(format::gibibytes_value(free + inactive)),
            },
            Sensor {
                id: "memory.total".into(),
                key: "total".into(),
                label: "Total".into(),
                reading: format::gibibytes(format::gibibytes_value(total)),
            },
        ];
        CategoryReading {
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
                self.host,
                PROCESSOR_CPU_LOAD_INFO,
                &mut cpu_count,
                &mut info,
                &mut info_count,
            )
        };
        if result != 0 || info.is_null() {
            return CategoryReading::empty();
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
        if self.core_descriptors.len() != per_core.len() {
            self.core_descriptors = (0..per_core.len())
                .map(|index| (format!("cpu{index}"), format!("Core {index}")))
                .collect();
        }
        for (index, value) in per_core.iter().enumerate() {
            let (key, label) = &self.core_descriptors[index];
            sensors.push(Sensor {
                id: format!("cpu.core.{index}"),
                key: key.clone(),
                label: label.clone(),
                reading: format::percent(*value),
            });
        }
        CategoryReading {
            sensors,
            average: format::percent(total_load),
        }
    }

    pub fn system_load(&self) -> CategoryReading {
        let mut loads = [0.0f64; 3];
        let count = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
        if count <= 0 {
            return CategoryReading::empty();
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
            sensors,
            average: format::load(loads[0]),
        }
    }

    pub fn network(&mut self) -> CategoryReading {
        let Some(counters) = interface_counters() else {
            return CategoryReading::empty();
        };
        let now = Instant::now();
        let mut received_rate = 0.0;
        let mut sent_rate = 0.0;
        if let Some((previous, prev_at)) = self.previous_network.take() {
            let elapsed = now.duration_since(prev_at).as_secs_f64();
            if elapsed > 0.05 {
                let mut received: u64 = 0;
                let mut sent: u64 = 0;
                for (name, (received_bytes, sent_bytes)) in &counters {
                    // The counters are 32 bits wide, so the difference has to
                    // wrap the same way they do; a missing interface (new
                    // connection, cable replug) has no delta at all.
                    if let Some((before_rx, before_tx)) = previous.get(name) {
                        received += received_bytes.wrapping_sub(*before_rx) as u64;
                        sent += sent_bytes.wrapping_sub(*before_tx) as u64;
                    }
                }
                received_rate = received as f64 / elapsed;
                sent_rate = sent as f64 / elapsed;
            }
        }
        self.previous_network = Some((counters, now));

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
            sensors,
            average,
        }
    }

    pub fn storage(&self) -> CategoryReading {
        let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
        let path = std::ffi::CString::new("/").unwrap();
        let rc = unsafe { libc::statfs(path.as_ptr(), &mut stats) };
        if rc != 0 {
            return CategoryReading::empty();
        }
        let block_size = stats.f_bsize as u64;
        let total = stats.f_blocks as u64 * block_size;
        let available = stats.f_bavail as u64 * block_size;
        if total == 0 {
            return CategoryReading::empty();
        }
        let used = total.saturating_sub(available);
        let used_percent = used as f64 / total as f64 * 100.0;

        let sensors = vec![
            Sensor {
                id: "storage.available".into(),
                key: "available".into(),
                label: "Available".into(),
                reading: format::gibibytes(format::gibibytes_value(available)),
            },
            Sensor {
                id: "storage.used".into(),
                key: "used".into(),
                label: "Used".into(),
                reading: format::gibibytes(format::gibibytes_value(used)),
            },
            Sensor {
                id: "storage.total".into(),
                key: "total".into(),
                label: "Total".into(),
                reading: format::gibibytes(format::gibibytes_value(total)),
            },
        ];
        CategoryReading {
            sensors,
            average: format::percent(used_percent),
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

fn interface_counters() -> Option<HashMap<String, (u32, u32)>> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return None;
    }
    let mut counters: HashMap<String, (u32, u32)> = HashMap::new();
    let mut cursor = head;
    while !cursor.is_null() {
        let entry = unsafe { &*cursor };
        let address = entry.ifa_addr;
        if !address.is_null() && unsafe { (*address).sa_family as i32 } == libc::AF_LINK
            && !entry.ifa_data.is_null()
        {
            // Only real interfaces: utun, bridge, awdl and friends would count
            // the same traffic twice.
            let name = unsafe { CStr::from_ptr(entry.ifa_name) }.to_string_lossy().into_owned();
            if name.starts_with("en") {
                let data = unsafe { &*(entry.ifa_data as *const IfDataHead) };
                let entry = counters.entry(name).or_insert((0, 0));
                entry.0 = entry.0.wrapping_add(data.ibytes);
                entry.1 = entry.1.wrapping_add(data.obytes);
            }
        }
        cursor = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(head) };
    Some(counters)
}
