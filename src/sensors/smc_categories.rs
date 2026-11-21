//! Temperature, voltage and fan, built from the SMC key table.

use crate::model::{format, CategoryReading, Sensor};

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
        return CategoryReading::empty();
    }
    let mean = sensors.iter().map(|s| s.reading.value).sum::<f64>() / sensors.len() as f64;
    CategoryReading {
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
        return CategoryReading::empty();
    }
    let mean = sensors.iter().map(|s| s.reading.value).sum::<f64>() / sensors.len() as f64;
    CategoryReading {
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
        return CategoryReading::empty();
    }
    let mean = sensors.iter().map(|s| s.reading.value).sum::<f64>() / sensors.len() as f64;
    CategoryReading {
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