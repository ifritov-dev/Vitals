//! The menu bar application: status items, the live dropdown and the
//! in-menu configuration.
//!
//! Objective-C needs a target object for the timer and the menu delegate, so
//! a class is registered at runtime whose methods recover the single-threaded
//! application state from a global.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::diag;
use crate::model::{Category, CategoryReading, ALL_CATEGORIES};
use crate::objc::{self, NSRunLoopCommonModes, NSSize, Obj, Sel};
use crate::prefs::Prefs;
use crate::store::SensorStore;

const TAG_QUIT: isize = -22;
const TAG_INTERVAL_BASE: isize = -30;

struct AppState {
    store: SensorStore,
    prefs: Prefs,
    controller: Obj,
    status_items: Vec<Obj>,
    /// Tag -> menu item, so open menus can be updated in place.
    live_items: HashMap<isize, Obj>,
    open_menu: Obj,
    tracking_timer: Obj,
    last_refresh: Instant,
    last_signature: String,
    /// Value column of the dropdown, fixed while the menu is open so a value
    /// that grows between ticks moves nothing.
    row_tab: f64,
    /// Same, per category, for each submenu.
    sensor_tabs: [f64; 9],
}

static STATE: Mutex<Option<AppState>> = Mutex::new(None);

// The app is single-threaded: every message, the timer and the menu all run
// on the main run loop, so the state is only ever touched from one thread.
unsafe impl Send for AppState {}

fn with_state<R>(body: impl FnOnce(&mut AppState) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|error| error.into_inner());
    let state = guard.as_mut().expect("state is installed before any message");
    body(state)
}

pub fn prepare_application() {
    unsafe {
        let _app = objc::send(objc::class("NSApplication"), "sharedApplication");
    }
}

pub fn run() {
    prepare_application();
    unsafe {
        let app = objc::send(objc::class("NSApplication"), "sharedApplication");
        // LSUIElement is in Info.plist; accessory keeps it true from a terminal.
        objc::send_void_int(app, "setActivationPolicy:", 1);
        let controller = make_controller();
        let mut store = SensorStore::new();
        store.refresh();
        let prefs = Prefs::load();

        *STATE.lock().unwrap() = Some(AppState {
            store,
            prefs,
            controller,
            status_items: Vec::new(),
            live_items: HashMap::new(),
            open_menu: std::ptr::null_mut(),
            tracking_timer: std::ptr::null_mut(),
            row_tab: 0.0,
            sensor_tabs: [0.0; 9],
            last_refresh: Instant::now(),
            last_signature: String::new(),
        });

        with_state(|state| rebuild_status_items(state));

        let timer = objc::send_timer(
            objc::class("NSTimer"),
            "timerWithTimeInterval:target:selector:userInfo:repeats:",
            0.5,
            controller,
            objc::selector("tick:"),
            std::ptr::null_mut(),
            true,
        );
        let run_loop = objc::send(objc::class("NSRunLoop"), "mainRunLoop");
        objc::send_void_obj_obj(run_loop, "addTimer:forMode:", timer, NSRunLoopCommonModes);

        if std::env::args().any(|a| a == "--demo-open") {
            let opener = objc::send_timer(
                objc::class("NSTimer"),
                "timerWithTimeInterval:target:selector:userInfo:repeats:",
                2.0,
                controller,
                objc::selector("demoOpen:"),
                std::ptr::null_mut(),
                false,
            );
            objc::send_void_obj_obj(run_loop, "addTimer:forMode:", opener, NSRunLoopCommonModes);
            let closer = objc::send_timer(
                objc::class("NSTimer"),
                "timerWithTimeInterval:target:selector:userInfo:repeats:",
                5.0,
                controller,
                objc::selector("demoClose:"),
                std::ptr::null_mut(),
                false,
            );
            objc::send_void_obj_obj(run_loop, "addTimer:forMode:", closer, NSRunLoopCommonModes);
            if std::env::args().any(|a| a == "--demo-actions") {
                let actions = objc::send_timer(
                    objc::class("NSTimer"),
                    "timerWithTimeInterval:target:selector:userInfo:repeats:",
                    3.0,
                    controller,
                    objc::selector("demoActions:"),
                    std::ptr::null_mut(),
                    false,
                );
                objc::send_void_obj_obj(run_loop, "addTimer:forMode:", actions, NSRunLoopCommonModes);
            }
        }

        objc::send_void(app, "run");
    }
}

// MARK: - Runtime class

unsafe extern "C" fn tick_imp(_this: Obj, _cmd: Sel, _timer: Obj) {
    with_state(|state| tick(state));
}

extern "C" fn tick_source(_context: *mut std::ffi::c_void) {
    with_state(|state| tick(state));
}

unsafe extern "C" fn menu_needs_update(_this: Obj, _cmd: Sel, menu: Obj) {
    with_state(|state| populate(state, menu));
}

unsafe extern "C" fn menu_will_open(_this: Obj, _cmd: Sel, menu: Obj) {
    with_state(|state| {
        state.open_menu = menu;
        // A menu tracks in NSEventTrackingRunLoopMode, where the regular
        // ticker does not run; this one keeps the open dropdown live.
        let controller = state.controller;
        let timer = objc::send_timer(
            objc::class("NSTimer"),
            "timerWithTimeInterval:target:selector:userInfo:repeats:",
            0.4,
            controller,
            objc::selector("tick:"),
            std::ptr::null_mut(),
            true,
        );
        let run_loop = objc::send(objc::class("NSRunLoop"), "mainRunLoop");
        objc::send_void_obj_obj(run_loop, "addTimer:forMode:", timer, objc::NSEventTrackingRunLoopMode);
        state.tracking_timer = objc::retain(timer);
    });
}

unsafe extern "C" fn menu_did_close(_this: Obj, _cmd: Sel, _menu: Obj) {
    with_state(|state| {
        if !state.tracking_timer.is_null() {
            objc::send_void(state.tracking_timer, "invalidate");
            state.tracking_timer = std::ptr::null_mut();
        }
        state.open_menu = std::ptr::null_mut();
        state.live_items.clear();
    });
}

/// Exercises the action paths the mouse would take, without a mouse.
unsafe extern "C" fn demo_actions(_this: Obj, _cmd: Sel, _timer: Obj) {
    with_state(|state| {
        eprintln!("demo: selecting a sensor for Temperature");
        dispatch(state, category_tag(Category::Temperature, Some(4)));
        eprintln!("demo: selecting Average again");
        dispatch(state, category_tag(Category::Temperature, None));
        eprintln!("demo: hiding and restoring the first category");
        let first = ALL_CATEGORIES[0];
        let active = state.prefs.selection(first).cloned();
        dispatch(state, category_tag(first, None));
        dispatch(state, category_tag(first, active.is_some().then_some(0)));
        eprintln!("demo: toggling a menu bar category");
        dispatch(state, -((Category::Battery.index() as isize) + 1));
        dispatch(state, -((Category::Battery.index() as isize) + 1));
        eprintln!("demo: changing the interval");
        dispatch(state, TAG_INTERVAL_BASE - 5);
        eprintln!("demo: actions done");
    });
}

unsafe extern "C" fn demo_close(_this: Obj, _cmd: Sel, _timer: Obj) {
    with_state(|state| {
        if !state.open_menu.is_null() {
            eprintln!("demo: cancelling tracking");
            objc::send_void(state.open_menu, "cancelTracking");
            eprintln!("demo: tracking cancelled");
        }
    });
}

unsafe extern "C" fn demo_open(_this: Obj, _cmd: Sel, _timer: Obj) {
    with_state(|state| {
        if let Some(item) = state.status_items.first().copied() {
            if let Some(button) = button_of(item) {
                eprintln!("demo: clicking status item");
                objc::send_void_obj(button, "performClick:", std::ptr::null_mut());
                eprintln!("demo: menu closed again");
            }
        }
    });
}

unsafe extern "C" fn handle_action(_this: Obj, _cmd: Sel, sender: Obj) {
    let tag = with_state(|_| unsafe { objc::send_int_result(sender, "tag") });
    with_state(|state| dispatch(state, tag));
}

fn make_controller() -> Obj {
    unsafe {
        let name = CString::new("VitalsController").unwrap();
        let cls = objc::objc_allocateClassPair(objc::class("NSObject"), name.as_ptr(), 0);
        let void_obj = CString::new("v@:@").unwrap();

        let methods: [(&str, *const ()); 8] = [
            ("tick:", tick_imp as *const ()),
            ("demoOpen:", demo_open as *const ()),
            ("demoClose:", demo_close as *const ()),
            ("demoActions:", demo_actions as *const ()),
            ("menuNeedsUpdate:", menu_needs_update as *const ()),
            ("menuWillOpen:", menu_will_open as *const ()),
            ("menuDidClose:", menu_did_close as *const ()),
            ("handleAction:", handle_action as *const ()),
        ];
        for (selector_name, implementation) in methods {
            let sel = objc::selector(selector_name);
            let types = void_obj.as_ptr();
            objc::class_addMethod(cls, sel, implementation as *const std::ffi::c_void, types);
        }
        objc::objc_registerClassPair(cls);
        let instance = objc::send(cls, "new");
        instance
    }
}

// MARK: - Status items

fn signature(prefs: &Prefs) -> String {
    format!(
        "{:?}",
        prefs.ordered_menu_bar().iter().map(|c| c.key()).collect::<Vec<_>>()
    )
}

fn rebuild_status_items(state: &mut AppState) {
    unsafe {
        for item in state.status_items.drain(..) {
            let bar = objc::send(objc::class("NSStatusBar"), "systemStatusBar");
            objc::send_void_obj(bar, "removeStatusItem:", item);
            objc::release(item);
        }
        let bar = objc::send(objc::class("NSStatusBar"), "systemStatusBar");
        let categories = state.prefs.ordered_menu_bar();

        // +statusItemWithLength: hands back an autoreleased item, and the
        // pointer we keep has to outlive the pool that created it.
        let item = objc::retain(objc::send_float(bar, "statusItemWithLength:", -1.0));
        let menu = make_menu(state.controller);
        objc::send_void_obj(item, "setMenu:", menu);
        state.status_items.push(item);
        let _ = categories;
        state.last_signature = signature(&state.prefs);
        update_status_titles(state);
    }
}

unsafe fn make_menu(controller: Obj) -> Obj {
    let menu = objc::send_obj(objc::alloc("NSMenu"), "initWithTitle:", objc::nsstring("Vitals"));
    objc::send_void_obj(menu, "setDelegate:", controller);
    objc::send_void_int(menu, "setAutoenablesItems:", 0);
    menu
}

unsafe fn button_of(item: Obj) -> Option<Obj> {
    let button = objc::send(item, "button");
    if button.is_null() {
        None
    } else {
        Some(button)
    }
}

fn update_status_titles(state: &mut AppState) {
    unsafe {
        let categories = state.prefs.ordered_menu_bar();
        {
            let Some(item) = state.status_items.first() else {
                return;
            };
            let Some(button) = button_of(*item) else { return };
            let mut title = Attributed::new();
            for (index, category) in categories.iter().enumerate() {
                if index > 0 {
                    title.append_string("  ");
                }
                let reading = state.store.get(*category);
                let value = state
                    .prefs
                    .selection(*category)
                    .map(|id| reading.reading_for(Some(id.as_str())).compact.clone())
                    .unwrap_or_else(|| reading.average.compact.clone());
                let value = if reading.reading_for(state.prefs.selection(*category).map(|s| s.as_str())).has_data() {
                    value
                } else {
                    "—".to_string()
                };
                title.append_symbol(*category, reading);
                title.append_string(&format!("\u{2009}{value}"));
            }
            if title.is_empty() {
                title.append_string("Vitals");
            }
            let finished = title.finish();
            objc::send_void_obj(button, "setAttributedTitle:", finished);
            objc::release(finished);
            let tooltip = categories
                .iter()
                .map(|category| {
                    let reading = state.store.get(*category);
                    format!(
                        "{}: {}",
                        category.title(),
                        reading.reading_for(state.prefs.selection(*category).map(|s| s.as_str())).text
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            objc::send_void_obj(button, "setToolTip:", objc::nsstring(&tooltip));
        }
    }
}

// MARK: - Tick

fn tick(state: &mut AppState) {
    let due = state.last_refresh.elapsed() >= Duration::from_secs_f64(state.prefs.interval);
    if due {
        state.store.refresh();
        state.last_refresh = Instant::now();
    }
    if signature(&state.prefs) != state.last_signature {
        rebuild_status_items(state);
        return;
    }
    update_status_titles(state);
    if !state.open_menu.is_null() {
        refresh_values(state);
    }
}

// MARK: - Menu construction

fn menu_item(title: &str, action: &str, tag: isize) -> Obj {
    unsafe {
        let selector = if action.is_empty() {
            std::ptr::null_mut()
        } else {
            objc::selector(action)
        };
        let item = objc::send_obj_sel_obj(
            objc::alloc("NSMenuItem"),
            "initWithTitle:action:keyEquivalent:",
            objc::nsstring(title),
            selector,
            objc::nsstring(""),
        );
        objc::send_void_int(item, "setTag:", tag);
        item
    }
}

fn action_item(title: &str, controller: Obj, tag: isize) -> Obj {
    unsafe {
        let item = menu_item(title, "handleAction:", tag);
        objc::send_void_obj(item, "setTarget:", controller);
        item
    }
}

unsafe fn set_row_title(item: Obj, left: &str, right: &str, tab_stop: f64, dim: bool) {
    let attributed = Attributed::row(left, right, tab_stop, dim);
    objc::send_void_obj(item, "setAttributedTitle:", attributed);
    // The item retains it; this drops the +1 we hold from -initWithAttributedString:.
    objc::release(attributed);
}

/// Right edge of the value column. Values are right-aligned here, so a short
/// value sits against the same edge as a long one; the widest value's own
/// width is part of the stop, otherwise the longest value would run back over
/// its label instead of ending at that edge.
/// Right edge of the value column.
fn column_right_for(labels: &[String], values: &[String]) -> f64 {
    let widest_label = widest_text(labels, false);
    let widest_value = widest_text(values, true);
    (widest_label.ceil() + COLUMN_GUTTER + widest_value.ceil()).ceil()
}

const COLUMN_GUTTER: f64 = 22.0;

/// Values are drawn in the monospaced-digit font, so they have to be measured
/// in it too; the menu font is narrower and would under-reserve the column.
fn widest_text(items: &[String], monospaced_digits: bool) -> f64 {
    unsafe {
        let font = if monospaced_digits {
            objc::send_obj_two_floats(
                objc::class("NSFont"),
                "monospacedDigitSystemFontOfSize:weight:",
                objc::system_font_size(),
                0.0,
            )
        } else {
            objc::send_float(objc::class("NSFont"), "menuFontOfSize:", 0.0)
        };
        let attributes = objc::send(objc::class("NSMutableDictionary"), "dictionary");
        objc::send_void_obj_obj(attributes, "setObject:forKey:", font, objc::nsstring("NSFont"));
        let mut widest: f64 = 0.0;
        for item in items {
            let text = objc::nsstring(item);
            widest = widest.max(text_width(text, attributes));
        }
        widest
    }
}

unsafe fn text_width(text: Obj, attributes: Obj) -> f64 {
    let size: NSSize = objc::send_size_obj(text, "sizeWithAttributes:", attributes);
    size.width
}

fn populate(state: &mut AppState, menu: Obj) {
    unsafe {
        objc::send_void(menu, "removeAllItems");
        state.live_items.clear();

        let labels: Vec<String> = ALL_CATEGORIES
            .iter()
            .map(|category| category.title().to_string())
            .collect();
        let values: Vec<String> = ALL_CATEGORIES
            .iter()
            .map(|category| {
                let reading = state.store.get(*category);
                let value =
                    reading.reading_for(state.prefs.selection(*category).map(|s| s.as_str()));
                if value.has_data() {
                    value.text.clone()
                } else {
                    "No data".to_string()
                }
            })
            .collect();
        state.row_tab = column_right_for(&labels, &values);
        let row_tab = state.row_tab;

        for category in ALL_CATEGORIES {
            let reading = state.store.get(category).clone();
            let item = menu_item(category.title(), "", 0);
            let value = reading.reading_for(state.prefs.selection(category).map(|s| s.as_str()));
            set_row_title(
                item,
                category.title(),
                if value.has_data() { &value.text } else { "No data" },
                row_tab,
                false,
            );
            if let Some(image) = symbol_image(category, &reading) {
                objc::send_void_obj(item, "setImage:", image);
            }
            let submenu = objc::send_obj(
                objc::alloc("NSMenu"),
                "initWithTitle:",
                objc::nsstring(category.title()),
            );
            objc::send_void_int(submenu, "setAutoenablesItems:", 0);
            populate_submenu(state, submenu, category, &reading);
            objc::send_void_obj(item, "setSubmenu:", submenu);
            objc::send_void_int(item, "setTag:", row_tag(category));
            objc::send_void_obj(menu, "addItem:", item);
            state.live_items.insert(row_tag(category), item);
        }

        objc::send_void_obj(menu, "addItem:", objc::send(objc::class("NSMenuItem"), "separatorItem"));

        // Configuration lives in the menu itself: no separate window.
        let interval_item = menu_item("Refresh Interval", "", 0);
        let interval_menu = objc::send_obj(
            objc::alloc("NSMenu"),
            "initWithTitle:",
            objc::nsstring("Refresh Interval"),
        );
        for seconds in 1..=10 {
            let item = action_item(
                &format!("{seconds} s"),
                state.controller,
                TAG_INTERVAL_BASE - seconds as isize,
            );
            let on = (state.prefs.interval.round() as isize) == seconds;
            objc::send_void_int(item, "setState:", if on { 1 } else { 0 });
            objc::send_void_obj(interval_menu, "addItem:", item);
        }
        objc::send_void_obj(interval_item, "setSubmenu:", interval_menu);
        objc::send_void_obj(menu, "addItem:", interval_item);

        objc::send_void_obj(menu, "addItem:", objc::send(objc::class("NSMenuItem"), "separatorItem"));
        let quit = action_item("Quit Vitals", state.controller, TAG_QUIT);
        objc::send_void_obj(quit, "setKeyEquivalent:", objc::nsstring("q"));
        objc::send_void_obj(menu, "addItem:", quit);
    }
}

fn populate_submenu(state: &mut AppState, menu: Obj, category: Category, reading: &CategoryReading) {
    unsafe {
        let active = state.prefs.selection(category).cloned();
        let labels: Vec<String> = std::iter::once("Average".to_string())
            .chain(reading.sensors.iter().map(|sensor| {
                if let Some(key) = display_key(sensor) {
                    format!("{key}  {}", sensor.label)
                } else {
                    sensor.label.clone()
                }
            }))
            .collect();
        let values: Vec<String> = std::iter::once(reading.average.text.clone())
            .chain(reading.sensors.iter().map(|sensor| sensor.reading.text.clone()))
            .collect();
        let tab = column_right_for(&labels, &values);
        state.sensor_tabs[category.index()] = tab;

        let average = action_item("Average", state.controller, category_tag(category, None));
        set_row_title(
            average,
            "Average",
            if reading.average.has_data() { &reading.average.text } else { "No data" },
            tab,
            true,
        );
        objc::send_void_int(average, "setState:", if active.is_none() { 1 } else { 0 });
        objc::send_void_obj(menu, "addItem:", average);
        state.live_items.insert(category_tag(category, None), average);

        if reading.sensors.is_empty() {
            objc::send_void_obj(menu, "addItem:", objc::send(objc::class("NSMenuItem"), "separatorItem"));
            let none = menu_item("No data", "", 0);
            objc::send_void_int(none, "setEnabled:", 0);
            objc::send_void_obj(menu, "addItem:", none);
            return;
        }

        objc::send_void_obj(menu, "addItem:", objc::send(objc::class("NSMenuItem"), "separatorItem"));
        for (index, sensor) in reading.sensors.iter().enumerate() {
            let tag = category_tag(category, Some(index));
            let item = action_item(&sensor.label, state.controller, tag);
            let left = match display_key(sensor) {
                Some(key) => format!("{key}  {}", sensor.label),
                None => sensor.label.clone(),
            };
            set_row_title(item, &left, &sensor.reading.text, tab, true);
            let on = active.as_deref() == Some(sensor.id.as_str());
            objc::send_void_int(item, "setState:", if on { 1 } else { 0 });
            objc::send_void_obj(menu, "addItem:", item);
            state.live_items.insert(tag, item);
        }
    }
}

/// Rewrites the value columns of the menu that is currently open.
fn refresh_values(state: &mut AppState) {
    unsafe {
        let row_tab = state.row_tab;
        for category in ALL_CATEGORIES {
            let reading = state.store.get(category);
            if let Some(item) = state.live_items.get(&row_tag(category)).copied() {
                let value = reading.reading_for(state.prefs.selection(category).map(|s| s.as_str()));
                set_row_title(
                    item,
                    category.title(),
                    if value.has_data() { &value.text } else { "No data" },
                    row_tab,
                    false,
                );
            }
            let sensor_tab = state.sensor_tabs[category.index()];
            if let Some(item) = state.live_items.get(&category_tag(category, None)).copied() {
                set_row_title(
                    item,
                    "Average",
                    if reading.average.has_data() { &reading.average.text } else { "No data" },
                    sensor_tab,
                    true,
                );
            }
            for (index, sensor) in reading.sensors.iter().enumerate() {
                if let Some(item) = state.live_items.get(&category_tag(category, Some(index))).copied() {
                    let left = match display_key(sensor) {
                        Some(key) => format!("{key}  {}", sensor.label),
                        None => sensor.label.clone(),
                    };
                    set_row_title(item, &left, &sensor.reading.text, sensor_tab, true);
                }
            }
        }
    }
}

fn display_key(sensor: &crate::model::Sensor) -> Option<&str> {
    if sensor.key.is_empty() || sensor.key.len() < 2 {
        return None;
    }
    if sensor.key.eq_ignore_ascii_case(&sensor.label) {
        return None;
    }
    Some(&sensor.key)
}

/// Key for a category's own row. It must not collide with the sensor tags,
/// because the Average entry carries `category_tag(category, None)` and the
/// two would otherwise overwrite each other in the live-item map.
fn row_tag(category: Category) -> isize {
    -1000 - (category.index() as isize)
}

fn category_tag(category: Category, sensor: Option<usize>) -> isize {
    ((category.index() as isize) + 1) * 1000 + sensor.map(|i| i as isize + 1).unwrap_or(0)
}

// MARK: - Actions

fn dispatch(state: &mut AppState, tag: isize) {
    if tag >= 1000 {
        let category_index = (tag / 1000 - 1) as usize;
        let sensor_index = (tag % 1000) as usize;
        let Some(category) = ALL_CATEGORIES.get(category_index).copied() else {
            return;
        };
        let reading = state.store.get(category);
        let selection = if sensor_index == 0 {
            None
        } else {
            reading
                .sensors
                .get(sensor_index - 1)
                .map(|sensor| sensor.id.clone())
        };
        state.prefs.toggle_selection(category, selection.as_deref());
        refresh_values(state);
        update_status_titles(state);
        if signature(&state.prefs) != state.last_signature {
            rebuild_status_items(state);
        }
        return;
    }
    if tag == 0 {
        return;
    }
    match tag {
        TAG_QUIT => unsafe {
            let app = objc::send(objc::class("NSApplication"), "sharedApplication");
            objc::send_void_obj(app, "terminate:", std::ptr::null_mut());
        },
        other if other <= -31 && other >= -40 => {
            let seconds = (-other - 30) as f64;
            state.prefs.set_interval(seconds);
        }
        _ => {}
    }
    if signature(&state.prefs) != state.last_signature {
        rebuild_status_items(state);
    } else {
        update_status_titles(state);
    }
}

// MARK: - Building attributed strings

struct Attributed {
    string: Obj,
    length: usize,
    empty: bool,
}

impl Attributed {
    fn new() -> Attributed {
        unsafe {
            let string = objc::send_obj(
                objc::alloc("NSMutableAttributedString"),
                "initWithString:",
                objc::nsstring(""),
            );
            Attributed {
                string,
                length: 0,
                empty: true,
            }
        }
    }

    fn append_string(&mut self, text: &str) {
        unsafe {
            let piece = objc::send_obj(
                objc::alloc("NSAttributedString"),
                "initWithString:",
                objc::nsstring(text),
            );
            objc::send_void_obj(self.string, "appendAttributedString:", piece);
            objc::release(piece);
            self.length += text.chars().count();
            self.empty = false;
        }
    }

    fn append_symbol(&mut self, category: Category, reading: &CategoryReading) {
        unsafe {
            let name = if category == Category::Battery {
                battery_symbol(reading.reading_for(None).value)
            } else {
                category.symbols().iter().copied().find(|name| system_image(name).is_some())
            };
            let Some(name) = name else { return };
            let Some(image) = system_image(name) else { return };
            objc::send_void_two_floats(image, "setSize:", 14.0, 14.0);
            let attachment = objc::send(objc::alloc("NSTextAttachment"), "init");
            objc::send_void_obj(attachment, "setImage:", image);
            // The class factory, not an initializer: there is no
            // -initWithAttachment: on NSAttributedString.
            let piece = objc::send_obj(
                objc::class("NSAttributedString"),
                "attributedStringWithAttachment:",
                attachment,
            );
            objc::send_void_obj(self.string, "appendAttributedString:", piece);
            // `piece` comes from a class factory and is autoreleased; only the
            // attachment we alloc/init'd is ours to release.
            objc::release(attachment);
            self.length += 1;
            self.empty = false;
        }
    }

    fn is_empty(&self) -> bool {
        self.empty
    }

    fn finish(self) -> Obj {
        unsafe {
            let attributed = objc::send_obj(
                objc::alloc("NSAttributedString"),
                "initWithAttributedString:",
                self.string,
            );
            objc::release(self.string);
            attributed
        }
    }

    /// Two-column row: label, a tab, then the value ending on `column_right`.
    ///
    /// The tab is placed where this row's value has to start, not on the
    /// column edge with right alignment. A right-aligned stop makes AppKit
    /// reserve the tab position *and* the value's width when it sizes the
    /// menu, which leaves a dead band between the values and the menu edge.
    fn row(left: &str, right: &str, column_right: f64, dim: bool) -> Obj {
        unsafe {
            let text = format!("{left}\t{right}");
            let value_font = objc::send_obj_two_floats(
                objc::class("NSFont"),
                "monospacedDigitSystemFontOfSize:weight:",
                objc::system_font_size(),
                0.0,
            );
            let value_width = text_width(objc::nsstring(right), {
                let attributes = objc::send(objc::class("NSMutableDictionary"), "dictionary");
                objc::send_void_obj_obj(attributes, "setObject:forKey:", value_font, objc::nsstring("NSFont"));
                attributes
            });
            let tab_stop = (column_right - value_width).max(0.0);
            let paragraph = objc::send(objc::alloc("NSMutableParagraphStyle"), "init");
            let tab = objc::send_tab(tab_stop);
            let tabs = objc::send(objc::class("NSMutableArray"), "array");
            objc::send_void_obj(tabs, "addObject:", tab);
            objc::send_void_obj(paragraph, "setTabStops:", tabs);
            objc::send_void_float(paragraph, "setDefaultTabInterval:", tab_stop);

            let string = objc::send_obj(
                objc::alloc("NSMutableAttributedString"),
                "initWithString:",
                objc::nsstring(&text),
            );
            let full_length = text.chars().count();
            objc::send_attr_range(
                string,
                "addAttribute:value:range:",
                objc::nsstring("NSParagraphStyle"),
                paragraph,
                0,
                full_length,
            );

            let menu_font = objc::send_float(objc::class("NSFont"), "menuFontOfSize:", 0.0);
            objc::send_attr_range(
                string,
                "addAttribute:value:range:",
                objc::nsstring("NSFont"),
                menu_font,
                0,
                full_length,
            );

            if dim {
                let value_start = left.chars().count() + 1;
                objc::send_attr_range(
                    string,
                    "addAttribute:value:range:",
                    objc::nsstring("NSFont"),
                    value_font,
                    value_start,
                    right.chars().count(),
                );
                let color = objc::send(objc::class("NSColor"), "secondaryLabelColor");
                objc::send_attr_range(
                    string,
                    "addAttribute:value:range:",
                    objc::nsstring("NSForegroundColor"),
                    color,
                    value_start,
                    right.chars().count(),
                );
            }

            objc::release(paragraph);
            let attributed = objc::send_obj(
                objc::alloc("NSAttributedString"),
                "initWithAttributedString:",
                string,
            );
            objc::release(string);
            attributed
        }
    }
}

fn battery_symbol(percent: f64) -> Option<&'static str> {
    Some(if percent >= 87.5 {
        "battery.100"
    } else if percent >= 62.5 {
        "battery.75"
    } else if percent >= 37.5 {
        "battery.50"
    } else if percent >= 12.5 {
        "battery.25"
    } else {
        "battery.0"
    })
}

fn system_image(name: &str) -> Option<Obj> {
    unsafe {
        let image = objc::send_obj_obj(
            objc::class("NSImage"),
            "imageWithSystemSymbolName:accessibilityDescription:",
            objc::nsstring(name),
            std::ptr::null_mut(),
        );
        if image.is_null() {
            None
        } else {
            Some(image)
        }
    }
}

fn symbol_image(category: Category, reading: &CategoryReading) -> Option<Obj> {
    let name = if category == Category::Battery {
        battery_symbol(reading.reading_for(None).value)
    } else {
        category.symbols().iter().copied().find(|name| system_image(name).is_some())
    }?;
    let image = system_image(name)?;
    unsafe {
        objc::send_void_two_floats(image, "setSize:", 14.0, 14.0);
        objc::send_void_int(image, "setTemplate:", 1);
    }
    Some(image)
}

/// Builds the dropdown and prints it, without creating any status item, so
/// the whole UI can be checked without anything appearing on screen.
pub fn dump_menu() -> String {
    prepare_application();
    unsafe {
        let controller = make_controller();
        let mut store = SensorStore::new();
        store.refresh();
        let prefs = Prefs::load();
        *STATE.lock().unwrap() = Some(AppState {
            store,
            prefs,
            controller,
            status_items: Vec::new(),
            live_items: HashMap::new(),
            open_menu: std::ptr::null_mut(),
            tracking_timer: std::ptr::null_mut(),
            row_tab: 0.0,
            sensor_tabs: [0.0; 9],
            last_refresh: Instant::now(),
            last_signature: String::new(),
        });

        let menu = make_menu(controller);
        let mut out = String::from("# Vitals menu structure (live NSMenu objects)\n");
        with_state(|state| {
            populate(state, menu);
            out.push_str(&format!(
                "# menu bar slots: {}\n# refresh interval: {} s\n# selections: {}\n",
                state
                    .prefs
                    .ordered_menu_bar()
                    .iter()
                    .map(|c| c.title())
                    .collect::<Vec<_>>()
                    .join(", "),
                state.prefs.interval,
                diag::selection_summary(&state.prefs)
            ));
            out.push_str(&format!(
                "\n## Columns (value column ends at {:.1} pt)\n",
                state.row_tab
            ));
            let font = objc::send_float(objc::class("NSFont"), "menuFontOfSize:", 0.0);
            let attrs = objc::send(objc::class("NSMutableDictionary"), "dictionary");
            objc::send_void_obj_obj(attrs, "setObject:forKey:", font, objc::nsstring("NSFont"));
            for category in ALL_CATEGORIES {
                let reading = state.store.get(category);
                let value =
                    reading.reading_for(state.prefs.selection(category).map(|s| s.as_str()));
                let text = if value.has_data() { &value.text } else { "No data" };
                let label_width = text_width(objc::nsstring(category.title()), attrs);
                let value_width = text_width(objc::nsstring(text), attrs);
                let row = Attributed::row(category.title(), text, state.row_tab, false);
                let row_width = objc::send_size(row, "size").width;
                objc::release(row);
                out.push_str(&format!(
                    "   {:<12} label ends {:>6.1}, value starts {:>6.1}, gap {:>5.1}, ends {:>6.1}, row {:>6.1}\n",
                    category.title(),
                    label_width,
                    state.row_tab - value_width,
                    state.row_tab - value_width - label_width,
                    state.row_tab,
                    row_width
                ));
            }
            let size = objc::send_size(menu, "size");
            out.push_str(&format!(
                "\n## Menu widths\n   dropdown: {:.0} × {:.0} pt\n",
                size.width, size.height
            ));
            let items = objc::send(menu, "itemArray");
            let count = objc::send_int_result(items, "count");
            for index in 0..count {
                let item = objc::send_int(items, "objectAtIndex:", index);
                if item.is_null() {
                    continue;
                }
                let submenu = objc::send(item, "submenu");
                if submenu.is_null() {
                    continue;
                }
                let title = objc::send(item, "title");
                let utf8 = objc::send(title, "UTF8String") as *const i8;
                let name = if utf8.is_null() {
                    "?".to_string()
                } else {
                    std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
                };
                let sub_size = objc::send_size(submenu, "size");
                out.push_str(&format!(
                    "   {:<20}{:.0} × {:.0} pt\n",
                    name, sub_size.width, sub_size.height
                ));
            }
            // Dump what an open menu looks like after a live refresh, so a
            // value update cannot quietly rewrite a row's label.
            refresh_values(state);
            out.push_str("\n## Row style\n");
            out.push_str(&row_style_report(96.0));
            out.push_str("\n## Dropdown\n");
            walk(menu, 1, &mut out);
        });
        out
    }
}

fn row_style_report(tab_stop: f64) -> String {
    unsafe {
        let mut out = String::new();
        let tab = objc::send_tab(tab_stop);
        let location = objc::send_float_result(tab, "location");
        let alignment = objc::send_int_result(tab, "alignment");
        out.push_str(&format!(
            "   tab stop: location={location:.1} pt alignment={alignment} (0 = left; each row places its own stop so its value ends on the column edge)\n"
        ));
        let row = Attributed::row("Temperature", "45.7 °C", tab_stop, true);
        let size = objc::send_size(row, "size");
        out.push_str(&format!(
            "   row \"Temperature<TAB>45.7 °C\" measures {:.1} pt wide\n",
            size.width
        ));
        let wide = Attributed::row("Temperature", "45.7 °C", 200.0, true);
        let wide_size = objc::send_size(wide, "size");
        out.push_str(&format!(
            "   same row with a 200 pt tab stop measures {:.1} pt — the tab stop is applied only if this grows\n",
            wide_size.width
        ));
        out
    }
}

unsafe fn walk(menu: Obj, depth: usize, out: &mut String) {
    let items = objc::send(menu, "itemArray");
    let count = objc::send_int_result(items, "count");
    let indent = "   ".repeat(depth);
    for index in 0..count {
        let item = objc::send_int(items, "objectAtIndex:", index);
        if item.is_null() {
            continue;
        }
        if objc::send_bool_result(item, "isSeparatorItem") {
            out.push_str(&format!("{indent}────────\n"));
            continue;
        }
        let state = objc::send_int_result(item, "state");
        let mark = if state == 1 { "[✓]" } else { "[ ]" };
        let title = objc::send(item, "title");
        let utf8 = objc::send(title, "UTF8String") as *const i8;
        let name = if utf8.is_null() {
            "?".to_string()
        } else {
            std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
        };
        let submenu = objc::send(item, "submenu");
        let arrow = if submenu.is_null() { "" } else { "  ›" };
        out.push_str(&format!("{indent}{mark} {name}{arrow}\n"));
        if !submenu.is_null() {
            walk(submenu, depth + 1, out);
        }
    }
}

/// Draws the dropdown's rows, exactly as the menu draws them, into one image.
/// Nothing appears on screen, so the column layout can be checked offscreen.
pub fn render_rows(path: &str) {
    prepare_application();
    unsafe {
        let controller = make_controller();
        let mut store = SensorStore::new();
        store.refresh();
        let prefs = Prefs::load();
        *STATE.lock().unwrap() = Some(AppState {
            store,
            prefs,
            controller,
            status_items: Vec::new(),
            live_items: HashMap::new(),
            open_menu: std::ptr::null_mut(),
            tracking_timer: std::ptr::null_mut(),
            row_tab: 0.0,
            sensor_tabs: [0.0; 9],
            last_refresh: Instant::now(),
            last_signature: String::new(),
        });
        with_state(|state| {
            let menu = make_menu(state.controller);
            populate(state, menu);
            let items = objc::send(menu, "itemArray");
            let count = objc::send_int_result(items, "count");
            let mut rows: Vec<(Obj, f64)> = Vec::new();
            let mut widest: f64 = 0.0;
            for index in 0..count {
                let item = objc::send_int(items, "objectAtIndex:", index);
                if item.is_null() || objc::send_bool_result(item, "isSeparatorItem") {
                    continue;
                }
                let title = objc::send(item, "attributedTitle");
                if title.is_null() {
                    continue;
                }
                let size = objc::send_size(title, "size");
                widest = widest.max(size.width);
                rows.push((title, size.height));
            }
            let width = widest.ceil() + 8.0;
            let height: f64 = rows.iter().map(|(_, h)| h + 6.0).sum::<f64>() + 8.0;
            let image = objc::send_obj_two_floats(objc::alloc("NSImage"), "initWithSize:", width, height);
            objc::send_void(image, "lockFocus");
            let mut y = height - 4.0;
            for (title, row_height) in rows.iter() {
                y -= row_height + 6.0;
                objc::send_void_two_floats(*title, "drawAtPoint:", 4.0, y);
            }
            objc::send_void(image, "unlockFocus");
            let tiff = objc::send(image, "TIFFRepresentation");
            let bitmap = objc::send_obj(
                objc::class("NSBitmapImageRep"),
                "imageRepWithData:",
                tiff,
            );
            let png = objc::send_obj_int_obj(
                bitmap,
                "representationUsingType:properties:",
                4,
                objc::send(objc::class("NSDictionary"), "dictionary"),
            );
            let wrote = objc::send_bool_obj_result(
                png,
                "writeToFile:atomically:",
                objc::nsstring(path),
                true,
            );
            println!("rendered {} rows, width {width:.0} pt, wrote={wrote}", rows.len());
            objc::release(image);
        });
    }
}
