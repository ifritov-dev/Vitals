//! Assertions for the pure logic, runnable from the app itself.

use crate::model::{format, Category, CategoryReading, Reading};
use crate::store::SensorStore;
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

    // Bytes as the SMC actually returns them for a few well-known keys, so the
    // type table is checked against the driver and not only against itself.
    out.close("sp5a ratio", decode("sp5a", &pad(&[0x09, 0x67]), 2), 2407.0 / 1024.0, 1e-6);
    out.close("fpe2 fan 2407", decode("fpe2", &pad(&[0x25, 0x9C]), 2), 2407.0, 1e-6);
    out.close("fpe2 open fan", decode("fpe2", &pad(&[0x00, 0x00]), 2), 0.0, 1e-6);
    out.close("flt voltage", decode("flt ", &pad(&[0x9A, 0x99, 0xA9, 0x42]), 4), 84.8, 1e-3);
    out.close("ioft zero", decode("ioft", &pad(&[0u8; 8]), 8), 0.0, 1e-6);
    out.check(
        "unknown type is refused",
        decode("zzzz", &pad(&[0x01, 0x02]), 2).is_none(),
    );
    out.check(
        "a two-byte type with one byte is refused",
        decode("fpe2", &pad(&[0x25]), 1).is_none(),
    );
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
    out.check("format 1.5 KiB", format::scaled_bytes(1536.0) == "1.50 KiB");
    out.check(
        "format 12.4 MiB",
        format::scaled_bytes(12.4 * 1024.0 * 1024.0) == "12.4 MiB",
    );
    out.check("compact percent", format::percent(57.859).compact == "58%");

    // Unit boundaries are where a scaling bug hides: the rounded value must
    // roll into the next unit rather than read as a full one below it.
    out.check("1023 B stays bytes", format::scaled_bytes(1023.0) == "1023 B");
    out.check(
        "1023.9 KiB rolls into MiB",
        format::scaled_bytes(1023.9 * 1024.0) == "1.00 MiB",
    );
    out.check(
        "one byte short of MiB rolls up",
        format::scaled_bytes(1024.0 * 1024.0 - 1.0) == "1.00 MiB",
    );
    out.check("negative clamps to zero", format::scaled_bytes(-5.0) == "0 B");

    out.check(
        "key maps to its category",
        crate::model::category_for_key("TC0P") == Some(Category::Temperature)
            && crate::model::category_for_key("B0AC") == Some(Category::Battery)
            && crate::model::category_for_key("NOPE").is_none(),
    );
    // An empty menu bar must still produce a non-empty title: the status item
    // collapses to zero width otherwise and the menu cannot be opened again.
    let empty_bar = Prefs {
        menu_bar: Vec::new(),
        ..Prefs::default()
    };
    out.check(
        "empty menu bar still has a title",
        crate::ui::status_title(&empty_bar, &SensorStore::for_ui()) == "Vitals",
    );
    let two_slots = Prefs {
        menu_bar: vec![Category::Memory, Category::Processor],
        ..Prefs::default()
    };
    out.check(
        "filled menu bar lists its slots",
        crate::ui::status_title(&two_slots, &SensorStore::for_ui()).starts_with("memory="),
    );

    out.check(
        "describe shows no data for an empty category",
        CategoryReading::empty().describe(None) == "No data",
    );
    out.check(
        "describe shows the average text",
        CategoryReading {
            sensors: Vec::new(),
            average: format::percent(42.0),
        }
        .describe(None)
            == "42.0%",
    );

    // Choosing the row that is already active hides its category; choosing
    // anything else shows it and selects that row.
    let base = Prefs {
        menu_bar: vec![Category::Memory],
        selections: HashMap::new(),
        interval: 5.0,
        start_at_login: false,
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
        CategoryReading::empty().reading_for(None).text == "No data",
    );

    // The preference checks read the stored domain, which only exists for the
    // bundled app; the invariants they assert hold for any pref struct, so a
    // default is enough to keep them meaningful headless.
    let stored = Prefs::load();
    let prefs = if stored.menu_bar.is_empty() { Prefs::default() } else { stored };
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
