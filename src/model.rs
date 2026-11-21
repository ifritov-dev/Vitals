//! Categories, readings and their display strings.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
        self as usize
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

    pub const COUNT: usize = ALL_CATEGORIES.len();

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

}

const _: () = {
    let mut index = 0;
    while index < ALL_CATEGORIES.len() {
        assert!(ALL_CATEGORIES[index] as usize == index);
        index += 1;
    }
};

/// The category an SMC key belongs to, by its first byte. This is the one
/// place the mapping lives: enumeration, the store's key filters and the
/// sensor builders all ask here. Battery keys are absent on purpose: the
/// power source description is the only source that reports it correctly.
pub fn category_for_key(key: &str) -> Option<Category> {
    match key.as_bytes().first()? {
        b'T' => Some(Category::Temperature),
        b'V' => Some(Category::Voltage),
        b'F' => Some(Category::Fan),
        _ => None,
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
    pub sensors: Vec<Sensor>,
    pub average: Reading,
}

impl CategoryReading {
    pub fn empty() -> CategoryReading {
        CategoryReading {
            sensors: Vec::new(),
            average: Reading::no_data(),
        }
    }

    /// The text a row shows for this category: the chosen sensor's, or the
    /// average, or "No data". Shared so the dump and the menu cannot drift.
    pub fn describe(&self, selection: Option<&str>) -> String {
        let reading = self.reading_for(selection);
        if reading.has_data() {
            reading.text.clone()
        } else {
            "No data".to_string()
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
    use super::{Category, Reading};

    /// One sensor's reading, formatted for its category.
    pub fn for_category(category: Category, value: f64) -> Reading {
        match category {
            Category::Temperature => temperature(value),
            Category::Voltage => voltage(value),
            Category::Fan => rpm(value),
            _ => percent(value),
        }
    }

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

    pub fn gibibytes(value: f64) -> Reading {
        Reading::new(value, format!("{value:.1} GiB"), format!("{value:.1}G"))
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

    /// Compact byte count in the same binary units the callers divide by, so
    /// 1536 bytes reads 1.50 KiB and 1023.9 KiB never prints as "1024 KiB".
    pub fn scaled_bytes(value: f64) -> String {
        let units = ["B", "KiB", "MiB", "GiB", "TiB"];
        let mut amount = if value.is_finite() && value > 0.0 { value } else { 0.0 };
        let mut index = 0;
        while amount.round() >= 1024.0 && index < units.len() - 1 {
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

    pub fn gibibytes_value(bytes: u64) -> f64 {
        bytes as f64 / 1_073_741_824.0
    }
}
