//! One snapshot of every category.

use std::collections::HashMap;

use crate::model::{category_for_key, format, Category, CategoryReading, Reading, ALL_CATEGORIES};
use crate::sensors::{self, SystemSensors};
use crate::smc::SmcService;

/// A failed or unhealthy driver connection is retried every this many polls,
/// not on every one: opening costs a round trip and is usually fine.
const RECONNECT_EVERY: u32 = 20;
/// A sensor whose key stops answering shows "No data" after this many
/// consecutive misses instead of its last good number forever.
const MISSES_BEFORE_NO_DATA: u8 = 3;
/// A category whose row shows its average reads this fraction of its family
/// per poll. The rest of the family keeps the value it last had, so the
/// average still covers every sensor while the driver sees an eighth of the
/// calls: the average is the one thing on the bar that costs real work.
const AVERAGE_STRIDE: usize = 8;

/// A finished poll, handed from the polling thread to the UI.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub readings: Vec<CategoryReading>,
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
    misses: HashMap<String, u8>,
    /// Latest value of every key the connection knows, in the order the driver
    /// enumerated them. A poll of an averaged category refills a slice of it.
    values: Vec<(String, f64)>,
    /// Key -> position in `values`, so a read lands in place.
    value_at: HashMap<String, usize>,
    /// Which slice of each family the next poll reads.
    rotation: usize,
    /// False until the first poll after a connection has read every key, so
    /// the average is never built from a family that has barely been sampled.
    filled: bool,
    polls_since_open: u32,
    pub total_keys: usize,
    pub polled_keys: usize,
}

impl SensorStore {
    pub fn new() -> SensorStore {
        let mut system = SystemSensors::new();
        system.prime();
        let mut store = SensorStore {
            smc: None,
            system,
            snapshot: ALL_CATEGORIES.map(|_| CategoryReading::empty()).to_vec(),
            misses: HashMap::new(),
            values: Vec::new(),
            value_at: HashMap::new(),
            rotation: 0,
            filled: false,
            // The first refresh retries, so a driver hiccup at launch costs
            // one poll rather than the SMC categories for the whole session.
            polls_since_open: RECONNECT_EVERY,
            total_keys: 0,
            polled_keys: 0,
        };
        store.ensure_connection();
        store
    }

    /// A store with no SMC connection, for the GUI: the readings arrive from
    /// the polling thread, which owns the driver connection.
    pub fn for_ui() -> SensorStore {
        SensorStore {
            smc: None,
            system: SystemSensors::new(),
            snapshot: ALL_CATEGORIES.map(|_| CategoryReading::empty()).to_vec(),
            misses: HashMap::new(),
            values: Vec::new(),
            value_at: HashMap::new(),
            rotation: 0,
            filled: false,
            polls_since_open: 0,
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
        }
    }

    pub fn apply(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot.readings;
    }

    /// Opens the driver when there is no connection, and drops one that has
    /// stopped answering so the next attempt can replace it.
    fn ensure_connection(&mut self) {
        self.polls_since_open = self.polls_since_open.saturating_add(1);
        if let Some(service) = self.smc.as_ref() {
            if service.healthy() {
                return;
            }
            self.smc = None;
            self.polls_since_open = RECONNECT_EVERY;
        }
        if self.polls_since_open < RECONNECT_EVERY {
            return;
        }
        self.polls_since_open = 0;
        // A connection that enumerates no keys is a failed open: keeping it
        // would leave every SMC category on "No data" for the session.
        let Some(service) = SmcService::open().filter(|service| !service.specs.is_empty()) else {
            return;
        };
        self.total_keys = service.total_keys;
        self.polled_keys = service.specs.len();
        self.values = service
            .specs
            .iter()
            .map(|spec| (spec.key.clone(), f64::NAN))
            .collect();
        self.value_at = self
            .values
            .iter()
            .enumerate()
            .map(|(index, (key, _))| (key.clone(), index))
            .collect();
        self.rotation = 0;
        self.filled = false;
        self.smc = Some(service);
    }

    fn read(&self, keep: &dyn Fn(&crate::smc::Spec) -> bool) -> Vec<(String, f64)> {
        self.smc
            .as_ref()
            .map(|service| service.read_values_where(keep))
            .unwrap_or_default()
    }

    pub fn refresh(&mut self) {
        self.refresh_categories(&ALL_CATEGORIES);
    }

    /// Refreshes what the menu bar shows. A category showing one sensor reads
    /// that single key; a category showing its average reads a rotating slice
    /// of its family and averages the slice with the values the rest of the
    /// family still holds.
    pub fn refresh_shown(&mut self, shown: &[(Category, Option<String>)]) {
        self.ensure_connection();
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

        // The first poll of a connection reads everything, so the average
        // starts from every sensor rather than from an eighth of them.
        let stride = if self.filled { AVERAGE_STRIDE } else { 1 };
        let slice = self.rotation % stride;
        let mut position = 0usize;
        let mut wanted: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Some(service) = self.smc.as_ref() {
            for spec in &service.specs {
                let Some(category) = category_for_key(&spec.key) else {
                    continue;
                };
                if exact.iter().any(|key| spec.key == *key) {
                    wanted.insert(spec.key.clone());
                } else if averages.contains(&category) {
                    if position % stride == slice {
                        wanted.insert(spec.key.clone());
                    }
                    position += 1;
                }
            }
        }
        let keep = |spec: &crate::smc::Spec| wanted.contains(spec.key.as_str());
        let values = self.read(&keep);
        self.absorb(&values);
        self.rotation = self.rotation.wrapping_add(1);
        self.filled = true;

        for (category, selection) in shown {
            let Some(id) = selection.as_deref().filter(|_| smc_category(*category)) else {
                let reading = reading_for(&mut self.system, *category, &self.values);
                self.snapshot[category.index()] = reading;
                continue;
            };
            let slot = category.index();
            let Some(sensor) = self.snapshot[slot].sensors.iter().position(|s| s.id == id) else {
                // The sensor list was rebuilt without this id; rebuild the
                // category rather than keep a reading nobody selected.
                let reading = reading_for(&mut self.system, *category, &self.values);
                self.snapshot[slot] = reading;
                continue;
            };
            match values.iter().find(|(key, _)| key == id).map(|(_, value)| *value) {
                Some(value) => {
                    self.misses.remove(id);
                    self.snapshot[slot].sensors[sensor].reading = format::for_category(*category, value);
                }
                None => {
                    let misses = self.misses.entry(id.to_string()).or_insert(0);
                    *misses = misses.saturating_add(1);
                    if *misses >= MISSES_BEFORE_NO_DATA {
                        self.snapshot[slot].sensors[sensor].reading = Reading::no_data();
                    }
                }
            }
        }
    }

    /// Folds a finished read into the cached values.
    fn absorb(&mut self, values: &[(String, f64)]) {
        for (key, value) in values {
            if let Some(index) = self.value_at.get(key) {
                self.values[*index].1 = *value;
            }
        }
    }

    /// Refreshes only the requested categories and keeps the last reading of
    /// the rest. The menu bar usually shows two or three categories, and SMC
    /// keys for the other six cost a driver call each.
    pub fn refresh_categories(&mut self, wanted: &[Category]) {
        self.ensure_connection();
        let keep = |spec: &crate::smc::Spec| match category_for_key(&spec.key) {
            Some(category) => wanted.contains(&category),
            None => false,
        };
        let values = self.read(&keep);
        self.absorb(&values);
        // Every key of every wanted category has just been read, so the
        // average can start from the whole set on the next narrow poll.
        self.filled = true;

        for category in ALL_CATEGORIES {
            if wanted.contains(&category) {
                let reading = reading_for(&mut self.system, category, &values);
                self.snapshot[category.index()] = reading;
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

/// One category's reading from the sensors that answer for it.
fn reading_for(
    system: &mut SystemSensors,
    category: Category,
    values: &[(String, f64)],
) -> CategoryReading {
    match category {
        Category::Temperature => sensors::temperature(values),
        Category::Voltage => sensors::voltage(values),
        Category::Fan => sensors::fan(values),
        Category::Memory => system.memory(),
        Category::Processor => system.processor(),
        Category::System => system.system_load(),
        Category::Network => system.network(),
        Category::Storage => system.storage(),
        Category::Battery => sensors::battery::battery(),
    }
}
