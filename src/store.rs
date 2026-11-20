//! One snapshot of every category.

use crate::model::{Category, CategoryReading, ALL_CATEGORIES};
use crate::sensors::{self, SystemSensors};
use crate::smc::SmcService;

/// A finished poll, handed from the polling thread to the UI.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub readings: Vec<CategoryReading>,
    pub total_keys: usize,
    pub polled_keys: usize,
}

fn smc_category(category: Category) -> bool {
    matches!(
        category,
        Category::Temperature | Category::Voltage | Category::Fan
    )
}

pub struct SensorStore {
    smc: Option<SmcService>,
    system: SystemSensors,
    snapshot: Vec<CategoryReading>,
    pub total_keys: usize,
    pub polled_keys: usize,
}

impl SensorStore {
    pub fn new() -> SensorStore {
        let smc = SmcService::open();
        let total_keys = smc.as_ref().map(|s| s.total_keys).unwrap_or(0);
        let polled_keys = smc.as_ref().map(|s| s.specs.len()).unwrap_or(0);
        let mut system = SystemSensors::new();
        system.prime();
        SensorStore {
            smc,
            system,
            snapshot: ALL_CATEGORIES
                .iter()
                .map(|c| CategoryReading::empty(*c))
                .collect(),
            total_keys,
            polled_keys,
        }
    }

    /// A store with no SMC connection, for the GUI: the readings arrive from
    /// the polling thread, which owns the driver connection.
    pub fn for_ui() -> SensorStore {
        SensorStore {
            smc: None,
            system: SystemSensors::new(),
            snapshot: ALL_CATEGORIES
                .iter()
                .map(|c| CategoryReading::empty(*c))
                .collect(),
            total_keys: 0,
            polled_keys: 0,
        }
    }

    pub fn smc_available(&self) -> bool {
        self.smc.is_some()
    }

    /// The current readings, to be applied to the UI's own store.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            readings: self.snapshot.clone(),
            total_keys: self.total_keys,
            polled_keys: self.polled_keys,
        }
    }

    pub fn apply(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot.readings;
        self.total_keys = snapshot.total_keys;
        self.polled_keys = snapshot.polled_keys;
    }

    pub fn refresh(&mut self) {
        self.refresh_categories(&ALL_CATEGORIES);
    }

    /// Refreshes what the menu bar shows. A category showing one sensor reads
    /// that single key; only a category showing its average needs the whole
    /// family, because the average is over all of them.
    pub fn refresh_shown(&mut self, shown: &[(Category, Option<String>)]) {
        let exact: Vec<&str> = shown
            .iter()
            .filter(|(category, sensor)| sensor.is_some() && smc_category(*category))
            .filter_map(|(_, sensor)| sensor.as_deref())
            .collect();
        let averages: Vec<Category> = shown
            .iter()
            .filter(|(category, sensor)| sensor.is_none() || !smc_category(*category))
            .map(|(category, _)| *category)
            .collect();

        let keep = |spec: &crate::smc::Spec| {
            if exact.iter().any(|key| spec.key == *key) {
                return true;
            }
            match spec.key.as_bytes().first() {
                Some(b'T') => averages.contains(&Category::Temperature),
                Some(b'V') => averages.contains(&Category::Voltage),
                Some(b'F') => averages.contains(&Category::Fan),
                Some(b'B') => averages.contains(&Category::Battery),
                _ => false,
            }
        };
        let values = self
            .smc
            .as_ref()
            .map(|service| service.read_values_where(&keep))
            .unwrap_or_default();

        for (category, selection) in shown {
            let Some(id) = selection.as_deref().filter(|_| smc_category(*category)) else {
                self.refresh_category(*category, &values);
                continue;
            };
            let Some(value) = values.iter().find(|(key, _)| key == id).map(|(_, v)| *v) else {
                continue;
            };
            let mut reading = self.snapshot[category.index()].clone();
            let Some(sensor) = reading.sensors.iter_mut().find(|sensor| sensor.id == id) else {
                continue;
            };
            sensor.reading = crate::model::format::for_category(*category, value);
            self.snapshot[category.index()] = reading;
        }
    }

    fn refresh_category(&mut self, category: Category, values: &[(String, f64)]) {
        let reading = match category {
            Category::Temperature => sensors::temperature(values),
            Category::Voltage => sensors::voltage(values),
            Category::Fan => sensors::fan(values),
            Category::Memory => self.system.memory(),
            Category::Processor => self.system.processor(),
            Category::System => self.system.system_load(),
            Category::Network => self.system.network(),
            Category::Storage => self.system.storage(),
            Category::Battery => sensors::battery::battery(values),
        };
        self.snapshot[category.index()] = reading;
    }

    /// Refreshes only the requested categories and keeps the last reading of
    /// the rest. The menu bar usually shows two or three categories, and SMC
    /// keys for the other six cost a driver call each.
    pub fn refresh_categories(&mut self, wanted: &[Category]) {
        let wants = |category: Category| wanted.contains(&category);
        let keep = |spec: &crate::smc::Spec| match spec.key.as_bytes().first() {
            Some(b'T') => wants(Category::Temperature),
            Some(b'V') => wants(Category::Voltage),
            Some(b'F') => wants(Category::Fan),
            Some(b'B') => wants(Category::Battery),
            _ => false,
        };
        let values = self
            .smc
            .as_ref()
            .map(|service| service.read_values_where(&keep))
            .unwrap_or_default();

        for category in ALL_CATEGORIES {
            if wants(category) {
                self.refresh_category(category, &values);
            }
        }
    }

    pub fn get(&self, category: Category) -> &CategoryReading {
        let index = category.index();
        self.snapshot
            .get(index)
            .unwrap_or_else(|| unreachable!("every category has a slot"))
    }
}
