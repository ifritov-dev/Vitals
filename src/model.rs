//! Categories, readings and their display strings.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Temperature,
    Voltage,
    Fan,
    Memory,
    Processor,
    System,
    Network,
    Storage,
    Battery,
}

pub const ALL_CATEGORIES: [Category; 9] = [
    Category::Temperature,
    Category::Voltage,
    Category::Fan,
    Category::Memory,
    Category::Processor,
    Category::System,
    Category::Network,
    Category::Storage,
    Category::Battery,
];

impl Category {
    pub fn index(self) -> usize {
        ALL_CATEGORIES.iter().position(|c| *c == self).unwrap_or(0)
    }

    pub fn key(self) -> &'static str {
        match self {
            Category::Temperature => "temperature",
            Category::Voltage => "voltage",
            Category::Fan => "fan",
            Category::Memory => "memory",
            Category::Processor => "processor",
            Category::System => "system",
            Category::Network => "network",
            Category::Storage => "storage",
            Category::Battery => "battery",
        }
    }

    pub fn from_key(key: &str) -> Option<Category> {
        ALL_CATEGORIES.iter().copied().find(|c| c.key() == key)
    }

    pub fn title(self) -> &'static str {
        match self {
            Category::Temperature => "Temperature",
            Category::Voltage => "Voltage",
            Category::Fan => "Fan",
            Category::Memory => "Memory",
            Category::Processor => "Processor",
            Category::System => "System",
            Category::Network => "Network",
            Category::Storage => "Storage",
            Category::Battery => "Battery",
        }
    }

    /// SF Symbol names, most specific first.
    pub fn symbols(self) -> &'static [&'static str] {
        match self {
            Category::Temperature => &["thermometer.medium", "thermometer"],
            Category::Voltage => &["bolt.fill", "bolt"],
            Category::Fan => &["fanblades.fill", "fanblades", "wind"],
            Category::Memory => &["memorychip.fill", "memorychip"],
            Category::Processor => &["cpu.fill", "cpu"],
            Category::System => &["gauge.with.dots.needle.50percent", "gauge.medium"],
            Category::Network => &["network", "globe"],
            Category::Storage => &["internaldrive.fill", "internaldrive"],
            Category::Battery => &["battery.100", "battery.75", "bolt.batteryblock"],
        }
    }
}

/// One number ready for display: full text for a menu row and the terse text
/// a menu bar slot uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub value: f64,
    pub text: String,
    pub compact: String,
}

impl Reading {
    pub fn new(value: f64, text: String, compact: String) -> Reading {
        Reading { value, text, compact }
    }

    pub fn no_data() -> Reading {
        Reading {
            value: f64::NAN,
            text: "No data".to_string(),
            compact: "—".to_string(),
        }
    }

    pub fn has_data(&self) -> bool {
        !self.value.is_nan()
    }
}

#[derive(Clone, Debug)]
pub struct Sensor {
    pub id: String,
    pub key: String,
    pub label: String,
    pub reading: Reading,
}

#[derive(Clone, Debug)]
pub struct CategoryReading {
    pub category: Category,
    pub sensors: Vec<Sensor>,
    pub average: Reading,
}

impl CategoryReading {
    pub fn empty(category: Category) -> CategoryReading {
        CategoryReading {
            category,
            sensors: Vec::new(),
            average: Reading::no_data(),
        }
    }

    /// The value a slot shows: the chosen sensor, or the category average.
    pub fn reading_for(&self, selection: Option<&str>) -> &Reading {
        if let Some(id) = selection {
            if let Some(sensor) = self.sensors.iter().find(|s| s.id == id) {
                return &sensor.reading;
            }
        }
        &self.average
    }
}

pub mod format {
    use super::Reading;

    pub fn temperature(celsius: f64) -> Reading {
        Reading::new(
            celsius,
            format!("{celsius:.1} °C"),
            format!("{celsius:.0}°"),
        )
    }

    pub fn voltage(volts: f64) -> Reading {
        let text = if volts.abs() < 10.0 {
            format!("{volts:.3} V")
        } else {
            format!("{volts:.2} V")
        };
        Reading::new(volts, text, format!("{volts:.2} V"))
    }

    pub fn rpm(value: f64) -> Reading {
        Reading::new(value, format!("{value:.0} RPM"), format!("{value:.0}"))
    }

    pub fn percent(value: f64) -> Reading {
        Reading::new(value, format!("{value:.1}%"), format!("{value:.0}%"))
    }

    pub fn gigabytes(value: f64) -> Reading {
        Reading::new(value, format!("{value:.1} GB"), format!("{value:.1}G"))
    }

    pub fn load(value: f64) -> Reading {
        Reading::new(value, format!("{value:.2}"), format!("{value:.2}"))
    }

    pub fn watts(value: f64) -> Reading {
        Reading::new(value, format!("{value:.1} W"), format!("{value:.1}W"))
    }

    pub fn amperes(value: f64) -> Reading {
        Reading::new(value, format!("{value:.2} A"), format!("{value:.2}A"))
    }

    pub fn bytes_per_second(value: f64) -> Reading {
        let text = format!("{}/s", scaled_bytes(value));
        Reading::new(value, text.clone(), text)
    }

    /// Compact byte count, e.g. 0 B, 812 B, 12.4 MB, 3.50 GB.
    pub fn scaled_bytes(value: f64) -> String {
        let units = ["B", "KB", "MB", "GB", "TB"];
        let mut amount = if value.is_finite() && value > 0.0 { value } else { 0.0 };
        let mut index = 0;
        while amount >= 1024.0 && index < units.len() - 1 {
            amount /= 1024.0;
            index += 1;
        }
        if index == 0 {
            format!("{amount:.0} B")
        } else if amount < 10.0 {
            format!("{amount:.2} {}", units[index])
        } else {
            format!("{amount:.1} {}", units[index])
        }
    }

    pub fn gigabytes_value(bytes: u64) -> f64 {
        bytes as f64 / 1_073_741_824.0
    }
}
