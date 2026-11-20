//! Preferences persisted through NSUserDefaults.

use std::collections::HashMap;

use crate::model::{Category, ALL_CATEGORIES};
use crate::objc::{self, Obj};

const KEY_CATEGORIES: &str = "menuBarCategories";
const KEY_SELECTIONS: &str = "categorySelections";
const KEY_INTERVAL: &str = "refreshInterval";
const KEY_SHOW_ICONS: &str = "showIconsInMenuBar";
const KEY_COMPACT: &str = "compactSingleSlot";

pub const INTERVAL_RANGE: (f64, f64) = (1.0, 10.0);
pub const DEFAULT_CATEGORIES: [Category; 3] = [Category::Memory, Category::System, Category::Processor];

#[derive(Clone, Debug)]
pub struct Prefs {
    pub menu_bar: Vec<Category>,
    pub selections: HashMap<String, String>,
    pub interval: f64,
    pub show_icons: bool,
    pub compact: bool,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            menu_bar: DEFAULT_CATEGORIES.to_vec(),
            selections: HashMap::new(),
            interval: 2.0,
            show_icons: true,
            compact: true,
        }
    }
}

fn defaults() -> Obj {
    unsafe { objc::send(objc::class("NSUserDefaults"), "standardUserDefaults") }
}

impl Prefs {
    pub fn load() -> Prefs {
        let mut prefs = Prefs::default();
        unsafe {
            let store = defaults();

            let categories = objc::send_obj(store, "objectForKey:", objc::nsstring(KEY_CATEGORIES));
            if !categories.is_null() {
                let count = objc::send_int_result(categories, "count");
                let mut parsed = Vec::new();
                for index in 0..count {
                    let item = objc::send_int(categories, "objectAtIndex:", index);
                    if item.is_null() {
                        continue;
                    }
                    let utf8 = objc::send(item, "UTF8String") as *const std::ffi::c_char;
                    if utf8.is_null() {
                        continue;
                    }
                    let key = std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned();
                    if let Some(category) = Category::from_key(&key) {
                        parsed.push(category);
                    }
                }
                prefs.menu_bar = parsed;
            }

            let selections = objc::send_obj(store, "objectForKey:", objc::nsstring(KEY_SELECTIONS));
            if !selections.is_null() {
                let keys = objc::send(selections, "allKeys");
                let count = objc::send_int_result(keys, "count");
                for index in 0..count {
                    let key_obj = objc::send_int(keys, "objectAtIndex:", index);
                    if key_obj.is_null() {
                        continue;
                    }
                    let value = objc::send_obj(selections, "objectForKey:", key_obj);
                    if value.is_null() {
                        continue;
                    }
                    let key_utf8 = objc::send(key_obj, "UTF8String") as *const std::ffi::c_char;
                    let value_utf8 = objc::send(value, "UTF8String") as *const std::ffi::c_char;
                    if key_utf8.is_null() || value_utf8.is_null() {
                        continue;
                    }
                    prefs.selections.insert(
                        std::ffi::CStr::from_ptr(key_utf8).to_string_lossy().into_owned(),
                        std::ffi::CStr::from_ptr(value_utf8).to_string_lossy().into_owned(),
                    );
                }
            }

            let interval = objc::send_float_obj_result(store, "doubleForKey:", objc::nsstring(KEY_INTERVAL));
            if interval >= INTERVAL_RANGE.0 && interval <= INTERVAL_RANGE.1 {
                prefs.interval = interval;
            }

            let has_icons = objc::send_obj(store, "objectForKey:", objc::nsstring(KEY_SHOW_ICONS));
            if !has_icons.is_null() {
                prefs.show_icons = objc::send_bool_result(has_icons, "boolValue");
            }
            let has_compact = objc::send_obj(store, "objectForKey:", objc::nsstring(KEY_COMPACT));
            if !has_compact.is_null() {
                prefs.compact = objc::send_bool_result(has_compact, "boolValue");
            }
        }
        prefs
    }

    pub fn save(&self) {
        unsafe {
            let store = defaults();

            let array = objc::send(objc::class("NSMutableArray"), "array");
            for category in &self.menu_bar {
                objc::send_void_obj(array, "addObject:", objc::nsstring(category.key()));
            }
            objc::send_void_obj_obj(store, "setObject:forKey:", array, objc::nsstring(KEY_CATEGORIES));

            let dictionary = objc::send(objc::class("NSMutableDictionary"), "dictionary");
            for (key, value) in &self.selections {
                objc::send_void_obj_obj(
                    dictionary,
                    "setObject:forKey:",
                    objc::nsstring(value),
                    objc::nsstring(key),
                );
            }
            objc::send_void_obj_obj(store, "setObject:forKey:", dictionary, objc::nsstring(KEY_SELECTIONS));

            let interval = objc::send_float(objc::class("NSNumber"), "numberWithDouble:", self.interval);
            objc::send_void_obj_obj(store, "setObject:forKey:", interval, objc::nsstring(KEY_INTERVAL));

            let icons = objc::send_int(
                objc::class("NSNumber"),
                "numberWithBool:",
                if self.show_icons { 1 } else { 0 },
            );
            objc::send_void_obj_obj(store, "setObject:forKey:", icons, objc::nsstring(KEY_SHOW_ICONS));

            let compact = objc::send_int(
                objc::class("NSNumber"),
                "numberWithBool:",
                if self.compact { 1 } else { 0 },
            );
            objc::send_void_obj_obj(store, "setObject:forKey:", compact, objc::nsstring(KEY_COMPACT));

            objc::send_bool_result(store, "synchronize");
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
        self.save();
    }

    pub fn toggle_category(&mut self, category: Category) {
        if self.menu_bar.contains(&category) {
            self.menu_bar.retain(|c| *c != category);
        } else {
            self.menu_bar.push(category);
        }
        self.save();
    }

    pub fn set_interval(&mut self, seconds: f64) {
        self.interval = seconds.clamp(INTERVAL_RANGE.0, INTERVAL_RANGE.1);
        self.save();
    }

    pub fn set_show_icons(&mut self, value: bool) {
        self.show_icons = value;
        self.save();
    }

    pub fn set_compact(&mut self, value: bool) {
        self.compact = value;
        self.save();
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
