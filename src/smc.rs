//! AppleSMC access over IOKit plus the data-type decoders.
//!
//! The wire format is an 80-byte struct whose layout must match the driver.
//! `#[repr(C)]` over the same fields reproduces it exactly; a layout mismatch
//! comes back as kIOReturnBadArgument (0xE00002C2) on every call.

use std::ffi::{c_char, c_void, CString};

const KERNEL_INDEX: u32 = 2;
const CMD_READ_BYTES: u8 = 5;
const CMD_READ_INDEX: u8 = 8;
const CMD_READ_KEYINFO: u8 = 9;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyDataVers {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyDataPLimit {
    version: u16,
    length: u16,
    cpu_limit: u32,
    gpu_limit: u32,
    mem_limit: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyDataKeyInfo {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KeyData {
    key: u32,
    vers: KeyDataVers,
    p_limit: KeyDataPLimit,
    key_info: KeyDataKeyInfo,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

impl Default for KeyData {
    fn default() -> Self {
        KeyData {
            key: 0,
            vers: KeyDataVers::default(),
            p_limit: KeyDataPLimit::default(),
            key_info: KeyDataKeyInfo::default(),
            result: 0,
            status: 0,
            data8: 0,
            data32: 0,
            bytes: [0; 32],
        }
    }
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(port: u32, dict: *mut c_void) -> u32;
    fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connection: *mut u32) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> i32;
    static mach_task_self_: u32;
}

pub struct SmcService {
    connection: u32,
    pub total_keys: usize,
    pub specs: Vec<Spec>,
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub key: String,
    pub data_type: String,
    pub size: usize,
}

impl SmcService {
    pub fn open() -> Option<SmcService> {
        assert_eq!(
            std::mem::size_of::<KeyData>(),
            80,
            "the SMC wire struct must stay 80 bytes"
        );
        unsafe {
            let name = CString::new("AppleSMC").ok()?;
            let matching = IOServiceMatching(name.as_ptr());
            if matching.is_null() {
                return None;
            }
            let service = IOServiceGetMatchingService(0, matching);
            if service == 0 {
                return None;
            }
            let mut connection: u32 = 0;
            let kr = IOServiceOpen(service, mach_task_self_, 0, &mut connection);
            IOObjectRelease(service);
            if kr != 0 {
                return None;
            }
            let mut service = SmcService {
                connection,
                total_keys: 0,
                specs: Vec::new(),
            };
            service.load_keys();
            Some(service)
        }
    }

    fn call(&self, input: &KeyData) -> Option<KeyData> {
        let mut output = KeyData::default();
        let mut output_size = std::mem::size_of::<KeyData>();
        let kr = unsafe {
            IOConnectCallStructMethod(
                self.connection,
                KERNEL_INDEX,
                input as *const KeyData as *const c_void,
                std::mem::size_of::<KeyData>(),
                &mut output as *mut KeyData as *mut c_void,
                &mut output_size,
            )
        };
        if kr != 0 || output.result != 0 {
            return None;
        }
        Some(output)
    }

    fn key_info(&self, key: &str) -> Option<(u32, String)> {
        let mut input = KeyData::default();
        input.key = four_cc(key)?;
        input.data8 = CMD_READ_KEYINFO;
        let output = self.call(&input)?;
        let raw = output.key_info.data_type;
        let bytes = [
            (raw >> 24) as u8,
            (raw >> 16) as u8,
            (raw >> 8) as u8,
            raw as u8,
        ];
        let text: String = bytes
            .iter()
            .map(|b| *b as char)
            .collect::<String>()
            .trim_end()
            .to_string();
        Some((output.key_info.data_size, text))
    }

    fn key_at(&self, index: u32) -> Option<String> {
        let mut input = KeyData::default();
        input.data8 = CMD_READ_INDEX;
        input.data32 = index;
        let output = self.call(&input)?;
        let k = output.key;
        let chars = [
            (k >> 24) as u8,
            (k >> 16) as u8,
            (k >> 8) as u8,
            k as u8,
        ];
        Some(chars.iter().map(|b| *b as char).collect())
    }

    fn load_keys(&mut self) {
        let Some((size, _)) = self.key_info("#KEY") else {
            return;
        };
        let Some(bytes) = self.read_bytes("#KEY", size) else {
            return;
        };
        let count = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        self.total_keys = count as usize;

        let mut specs = Vec::new();
        for index in 0..count {
            let Some(key) = self.key_at(index) else { continue };
            if !is_interesting(&key) {
                continue;
            }
            if let Some((size, data_type)) = self.key_info(&key) {
                if size == 0 || size > 32 {
                    continue;
                }
                specs.push(Spec {
                    key,
                    data_type,
                    size: size as usize,
                });
            }
        }
        self.specs = specs;
    }

    fn read_bytes(&self, key: &str, size: u32) -> Option<[u8; 32]> {
        if size == 0 || size > 32 {
            return None;
        }
        let mut input = KeyData::default();
        input.key = four_cc(key)?;
        input.data8 = CMD_READ_BYTES;
        input.key_info.data_size = size;
        let output = self.call(&input)?;
        Some(output.bytes)
    }

    /// Re-reads every cached key: key -> value.
    pub fn read_values(&self) -> Vec<(String, f64)> {
        self.read_values_where(&|_| true)
    }

    /// Reads the keys a filter accepts. Each key is two calls into the driver,
    /// so reading only the categories that are on screen is most of the work
    /// saved on an idle machine.
    pub fn read_values_where(&self, keep: &dyn Fn(&Spec) -> bool) -> Vec<(String, f64)> {
        let mut values = Vec::with_capacity(self.specs.len());
        for spec in self.specs.iter().filter(|spec| keep(spec)) {
            let Some(bytes) = self.read_bytes(&spec.key, spec.size as u32) else {
                continue;
            };
            if let Some(value) = decode(&spec.data_type, &bytes, spec.size) {
                values.push((spec.key.clone(), value));
            }
        }
        values
    }
}

impl Drop for SmcService {
    fn drop(&mut self) {
        unsafe {
            IOServiceClose(self.connection);
        }
    }
}

fn four_cc(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    if bytes.len() != 4 {
        return None;
    }
    Some(
        (bytes[0] as u32) << 24
            | (bytes[1] as u32) << 16
            | (bytes[2] as u32) << 8
            | bytes[3] as u32,
    )
}

/// Only these keys are polled after startup.
fn is_interesting(key: &str) -> bool {
    let b = key.as_bytes();
    if b.len() != 4 {
        return false;
    }
    if b[0] == b'T' || b[0] == b'V' {
        return true;
    }
    if b[0] == b'F' && b[2] == b'A' && b[3] == b'c' && b[1].is_ascii_digit() {
        return true;
    }
    matches!(key, "B0AV" | "B0AC")
}

/// Decodes one payload according to its SMC type string.
///
/// `sp78` is *signed* 7.8 fixed point and `fpe2` is *unsigned* 14.2; getting
/// those two sign conventions the same way round is the classic bug here.
pub fn decode(data_type: &str, bytes: &[u8; 32], size: usize) -> Option<f64> {
    let kind = data_type.trim();
    match kind {
        "flt" if size >= 4 => {
            let bits = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            Some(f32::from_bits(bits) as f64)
        }
        "ioft" if size >= 8 => {
            let bits = u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Some(f64::from_bits(bits))
        }
        "sp78" => signed_fixed(bytes, size, 256.0),
        "sp87" => signed_fixed(bytes, size, 128.0),
        "sp5a" => signed_fixed(bytes, size, 1024.0),
        "sp96" => signed_fixed(bytes, size, 64.0),
        "sp4b" => signed_fixed(bytes, size, 2048.0),
        "fpe2" if size >= 2 => Some(u16::from_be_bytes([bytes[0], bytes[1]]) as f64 / 4.0),
        "fp1f" if size >= 2 => Some(u16::from_be_bytes([bytes[0], bytes[1]]) as f64 / 32768.0),
        "ui8" | "flag" if size >= 1 => Some(bytes[0] as f64),
        "ui16" if size >= 2 => Some(u16::from_be_bytes([bytes[0], bytes[1]]) as f64),
        "ui32" if size >= 4 => Some(
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
        ),
        "si8" if size >= 1 => Some(bytes[0] as i8 as f64),
        "si16" if size >= 2 => Some(i16::from_be_bytes([bytes[0], bytes[1]]) as f64),
        "si32" if size >= 4 => Some(
            i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
        ),
        _ => None,
    }
}

fn signed_fixed(bytes: &[u8; 32], size: usize, divisor: f64) -> Option<f64> {
    if size < 2 {
        return None;
    }
    let raw = i16::from_be_bytes([bytes[0], bytes[1]]);
    Some(raw as f64 / divisor)
}
