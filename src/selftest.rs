//! Assertions for the pure logic, runnable from the app itself.

use crate::model::{format, Category, CategoryReading, Reading};
use crate::prefs::Prefs;
use std::collections::HashMap;
use crate::smc::decode;

pub struct Outcome {
    pub passed: usize,
    pub failures: Vec<String>,
}

impl Outcome {
    fn check(&mut self, name: &str, condition: bool) {
        if condition {
            self.passed += 1;
        } else {
            self.failures.push(name.to_string());
        }
    }

    fn close(&mut self, name: &str, actual: Option<f64>, expected: f64, tolerance: f64) {
        match actual {
            Some(value) if (value - expected).abs() <= tolerance => self.passed += 1,
            Some(value) => self.failures.push(format!("{name}: got {value}, expected {expected}")),
            None => self.failures.push(format!("{name}: got nil, expected {expected}")),
        }
    }
}

fn pad(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..bytes.len()].copy_from_slice(bytes);
    out
}

pub fn run() -> Outcome {
    let mut out = Outcome {
        passed: 0,
        failures: Vec::new(),
    };

    // sp78 is signed 7.8 fixed point; the sign bit is what usually breaks.
    out.close("sp78 positive", decode("sp78", &pad(&[0x34, 0x12]), 2), 52.0703125, 1e-4);
    out.close("sp78 negative", decode("sp78", &pad(&[0xC0, 0x00]), 2), -64.0, 1e-4);
    out.close("sp78 minus one", decode("sp78", &pad(&[0xFF, 0x00]), 2), -1.0, 1e-4);
    // fpe2 is unsigned 14.2 fixed point.
    out.close("fpe2 768", decode("fpe2", &pad(&[0x0C, 0x00]), 2), 768.0, 1e-4);
    out.close("fpe2 1.0", decode("fpe2", &pad(&[0x00, 0x04]), 2), 1.0, 1e-4);
    // Type strings arrive padded to four characters.
    out.close(
        "padded flt",
        decode("flt ", &pad(&[0xBE, 0x8D, 0x5F, 0x42]), 4),
        55.888,
        0.01,
    );
    out.close("padded ui16", decode("ui16 ", &pad(&[0x0E, 0x2F]), 2), 3631.0, 1e-6);
    out.close("ui8", decode("ui8", &pad(&[0xFF]), 1), 255.0, 1e-6);
    out.close("si8", decode("si8", &pad(&[0xFF]), 1), -1.0, 1e-6);
    out.close("si16", decode("si16", &pad(&[0xFF, 0xFF]), 2), -1.0, 1e-6);
    out.close(
        "ui32 key count",
        decode("ui32", &pad(&[0x00, 0x00, 0x08, 0x85]), 4),
        2181.0,
        1e-6,
    );
    out.close(
        "ioft double",
        decode(
            "ioft",
            &pad(&71.5f64.to_le_bytes()),
            8,
        ),
        71.5,
        1e-9,
    );
    out.check("unknown type is nil", decode("ch8*", &pad(&[1, 2, 3, 4]), 4).is_none());
    out.check("short flt is nil", decode("flt", &pad(&[0, 0]), 2).is_none());

    out.check("format 0 B", format::scaled_bytes(0.0) == "0 B");
    out.check("format 812 B", format::scaled_bytes(812.0) == "812 B");
    out.check("format 1.5 KB", format::scaled_bytes(1536.0) == "1.50 KB");
    out.check(
        "format 12.4 MB",
        format::scaled_bytes(12.4 * 1024.0 * 1024.0) == "12.4 MB",
    );
    out.check("compact percent", format::percent(57.859).compact == "58%");

    // Choosing the row that is already active hides its category; choosing
    // anything else shows it and selects that row.
    let base = Prefs {
        menu_bar: vec![Category::Memory],
        selections: HashMap::new(),
        interval: 5.0,
    };
    let shown = base.toggled(Category::Processor, Some("PROC_0"));
    out.check(
        "picking a sensor shows its category",
        shown.menu_bar.contains(&Category::Processor)
            && shown.selection(Category::Processor).map(|s| s.as_str()) == Some("PROC_0"),
    );
    let again = shown.toggled(Category::Processor, Some("PROC_0"));
    out.check(
        "picking the active sensor hides its category",
        !again.menu_bar.contains(&Category::Processor) && again.selection(Category::Processor).is_none(),
    );
    let average = base.toggled(Category::Memory, None);
    out.check(
        "picking Average on a shown category hides it",
        !average.menu_bar.contains(&Category::Memory),
    );
    let other = shown.toggled(Category::Processor, Some("PROC_1"));
    out.check(
        "picking another sensor keeps the category and moves the check",
        other.menu_bar.contains(&Category::Processor)
            && other.selection(Category::Processor).map(|s| s.as_str()) == Some("PROC_1"),
    );
    out.check("percent text", format::percent(57.859).text == "57.9%");
    out.check(
        "temperature text",
        format::temperature(51.82).text == "51.8 °C",
    );
    out.check("no data marker", !Reading::no_data().has_data());
    out.check(
        "empty category says no data",
        CategoryReading::empty(Category::Fan).reading_for(None).text == "No data",
    );

    let prefs = Prefs::load();
    out.check("interval inside range", prefs.interval >= 1.0 && prefs.interval <= 10.0);
    out.check(
        "menu bar slots are known categories",
        prefs
            .ordered_menu_bar()
            .iter()
            .all(|c| Category::from_key(c.key()).is_some()),
    );

    out
}
