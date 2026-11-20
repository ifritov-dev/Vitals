//! One snapshot of every category.

use crate::model::{Category, CategoryReading, ALL_CATEGORIES};
use crate::sensors::{self, SystemSensors};
use crate::smc::SmcService;

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

    pub fn smc_available(&self) -> bool {
        self.smc.is_some()
    }

    pub fn refresh(&mut self) {
        let values = self
            .smc
            .as_ref()
            .map(|service| service.read_values())
            .unwrap_or_default();

        let mut fresh = Vec::with_capacity(ALL_CATEGORIES.len());
        fresh.push(sensors::temperature(&values));
        fresh.push(sensors::voltage(&values));
        fresh.push(sensors::fan(&values));
        fresh.push(self.system.memory());
        fresh.push(self.system.processor());
        fresh.push(self.system.system_load());
        fresh.push(self.system.network());
        fresh.push(self.system.storage());
        fresh.push(self.system.battery(&values));
        self.snapshot = fresh;
    }

    pub fn get(&self, category: Category) -> &CategoryReading {
        let index = category.index();
        self.snapshot
            .get(index)
            .unwrap_or_else(|| unreachable!("every category has a slot"))
    }

    pub fn all(&self) -> &[CategoryReading] {
        &self.snapshot
    }
}
