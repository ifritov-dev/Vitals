//! Preferences persisted through NSUserDefaults.

use std::collections::HashMap;

use crate::model::{Category, ALL_CATEGORIES};
use crate::objc::{self, Obj};

const KEY_CATEGORIES: &str = "menuBarCategories";
const KEY_SELECTIONS: &str = "categorySelections";
const KEY_INTERVAL: &str = "refreshInterval";
const KEY_LOGIN: &str = "startAtLogin";
const KEY_FIXED_WIDTH: &str = "fixedStatusWidth";

pub const INTERVAL_RANGE: (f64, f64) = (1.0, 10.0);
pub const DEFAULT_INTERVAL: f64 = 5.0;
pub const DEFAULT_CATEGORIES: [Category; 3] = [Category::Memory, Category::System, Category::Processor];

#[derive(Clone, Debug)]
pub struct Prefs {
    pub menu_bar: Vec<Category>,
    pub selections: HashMap<String, String>,
    pub interval: f64,
    pub start_at_login: bool,
    /// Pin the menu bar item to the width its categories need, instead of
    /// letting it size itself to whatever the readings currently say.
    pub fixed_status_width: bool,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            menu_bar: DEFAULT_CATEGORIES.to_vec(),
            selections: HashMap::new(),
            interval: DEFAULT_INTERVAL,
            start_at_login: false,
            fixed_status_width: true,
        }
    }
}

/// The string an element of a stored collection holds, or None when it is not
/// a string at all.
unsafe fn string_of(value: Obj) -> Option<String> {
    if value.is_null() || !objc::send_bool_obj(value, "isKindOfClass:", objc::class("NSString")) {
        return None;
    }
    let utf8 = objc::send(value, "UTF8String") as *const std::ffi::c_char;
    if utf8.is_null() {
        return None;
    }
    Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
}

fn defaults() -> Obj {
    unsafe { objc::send(objc::class("NSUserDefaults"), "standardUserDefaults") }
}

impl Prefs {
    pub fn load() -> Prefs {
        let mut prefs = Prefs::default();
        unsafe {
            let store = defaults();

            // Typed accessors hand back nil instead of raising on a value of
            // the wrong shape, so a stale or hand-edited domain leaves the
            // defaults in place rather than killing the launch.
            let categories = objc::send_obj(store, "arrayForKey:", objc::nsstring(KEY_CATEGORIES));
            if !categories.is_null() {
                let count = objc::send_int_result(categories, "count");
                let mut parsed = Vec::new();
                for index in 0..count {
                    let item = objc::send_int(categories, "objectAtIndex:", index);
                    if let Some(category) = string_of(item).and_then(|key| Category::from_key(&key)) {
                        parsed.push(category);
                    }
                }
                // An array that parses to nothing is a stale shape, not an
                // empty menu bar, so the defaults stay.
                if !parsed.is_empty() {
                    prefs.menu_bar = parsed;
                }
            }

            let selections = objc::send_obj(store, "dictionaryForKey:", objc::nsstring(KEY_SELECTIONS));
            if !selections.is_null() {
                let keys = objc::send(selections, "allKeys");
                let count = objc::send_int_result(keys, "count");
                for index in 0..count {
                    let key_obj = objc::send_int(keys, "objectAtIndex:", index);
                    let value = objc::send_obj(selections, "objectForKey:", key_obj);
                    if let (Some(key), Some(value)) = (string_of(key_obj), string_of(value)) {
                        prefs.selections.insert(key, value);
                    }
                }
            }

            let interval = objc::send_float_obj_result(store, "doubleForKey:", objc::nsstring(KEY_INTERVAL));
            if interval >= INTERVAL_RANGE.0 && interval <= INTERVAL_RANGE.1 {
                prefs.interval = interval;
            }

            let fixed = objc::send_obj(store, "objectForKey:", objc::nsstring(KEY_FIXED_WIDTH));
            if !fixed.is_null() && objc::send_bool_obj(fixed, "isKindOfClass:", objc::class("NSNumber")) {
                prefs.fixed_status_width = objc::send_bool_result(fixed, "boolValue");
            }

            let login = objc::send_obj(store, "objectForKey:", objc::nsstring(KEY_LOGIN));
            if !login.is_null() && objc::send_bool_obj(login, "isKindOfClass:", objc::class("NSNumber")) {
                prefs.start_at_login = objc::send_bool_result(login, "boolValue");
            }

        }
        prefs
    }

    /// Writes only the keys a caller actually changed: two instances of the
    /// app can then keep different settings without overwriting each other.
    fn save_categories(&self) {
        unsafe {
            let array = objc::send(objc::class("NSMutableArray"), "array");
            for category in &self.menu_bar {
                objc::send_void_obj(array, "addObject:", objc::nsstring(category.key()));
            }
            objc::send_void_obj_obj(
                defaults(),
                "setObject:forKey:",
                array,
                objc::nsstring(KEY_CATEGORIES),
            );
        }
    }

    fn save_selections(&self) {
        unsafe {
            let dictionary = objc::send(objc::class("NSMutableDictionary"), "dictionary");
            for (key, value) in &self.selections {
                objc::send_void_obj_obj(
                    dictionary,
                    "setObject:forKey:",
                    objc::nsstring(value),
                    objc::nsstring(key),
                );
            }
            objc::send_void_obj_obj(
                defaults(),
                "setObject:forKey:",
                dictionary,
                objc::nsstring(KEY_SELECTIONS),
            );
        }
    }

    fn save_interval(&self) {
        unsafe {
            let interval = objc::send_float(objc::class("NSNumber"), "numberWithDouble:", self.interval);
            objc::send_void_obj_obj(
                defaults(),
                "setObject:forKey:",
                interval,
                objc::nsstring(KEY_INTERVAL),
            );
        }
    }

    fn save_fixed_width(&self) {
        unsafe {
            let value = objc::send_int(
                objc::class("NSNumber"),
                "numberWithBool:",
                if self.fixed_status_width { 1 } else { 0 },
            );
            objc::send_void_obj_obj(
                defaults(),
                "setObject:forKey:",
                value,
                objc::nsstring(KEY_FIXED_WIDTH),
            );
        }
    }

    fn save_login(&self) {
        unsafe {
            let login = objc::send_int(
                objc::class("NSNumber"),
                "numberWithBool:",
                if self.start_at_login { 1 } else { 0 },
            );
            objc::send_void_obj_obj(
                defaults(),
                "setObject:forKey:",
                login,
                objc::nsstring(KEY_LOGIN),
            );
        }
    }

    pub fn selection(&self, category: Category) -> Option<&String> {
        self.selections.get(category.key())
    }

    pub fn set_selection(&mut self, category: Category, sensor: Option<&str>) {
        match sensor {
            Some(id) => {
                self.selections.insert(category.key().to_string(), id.to_string());
            }
            None => {
                self.selections.remove(category.key());
            }
        }
        if !self.menu_bar.contains(&category) {
            self.menu_bar.push(category);
        }
        self.save_categories();
        self.save_selections();
    }

    /// Picking a row in a category's submenu is how a category is shown or
    /// hidden: choosing the row that is already active takes the category out
    /// of the menu bar, choosing any other row puts it there.
    pub fn toggled(&self, category: Category, sensor: Option<&str>) -> Prefs {
        let mut next = self.clone();
        let active = next.selections.get(category.key()).map(|id| id.as_str());
        if next.menu_bar.contains(&category) && active == sensor {
            next.menu_bar.retain(|c| *c != category);
            next.selections.remove(category.key());
            return next;
        }
        match sensor {
            Some(id) => {
                next.selections.insert(category.key().to_string(), id.to_string());
            }
            None => {
                next.selections.remove(category.key());
            }
        }
        if !next.menu_bar.contains(&category) {
            next.menu_bar.push(category);
        }
        next
    }

    pub fn toggle_selection(&mut self, category: Category, sensor: Option<&str>) {
        *self = self.toggled(category, sensor);
        self.save_categories();
        self.save_selections();
    }



    pub fn set_fixed_status_width(&mut self, enabled: bool) {
        self.fixed_status_width = enabled;
        self.save_fixed_width();
    }

    pub fn set_start_at_login(&mut self, enabled: bool) {
        self.start_at_login = enabled;
        self.save_login();
    }

    pub fn set_interval(&mut self, seconds: f64) {
        self.interval = seconds.clamp(INTERVAL_RANGE.0, INTERVAL_RANGE.1);
        self.save_interval();
    }

    /// Ordered slots, in the canonical category order.
    pub fn ordered_menu_bar(&self) -> Vec<Category> {
        ALL_CATEGORIES
            .iter()
            .copied()
            .filter(|c| self.menu_bar.contains(c))
            .collect()
    }
}
