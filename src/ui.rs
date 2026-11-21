//! The menu bar application: status items, the live dropdown and the
//! in-menu configuration.
//!
//! Objective-C needs a target object for the timer and the menu delegate, so
//! a class is registered at runtime whose methods recover the single-threaded
//! application state from a global.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::diag;
use crate::model::{Category, CategoryReading, ALL_CATEGORIES};
use crate::poller::Polled;
use crate::objc::{self, NSRunLoopCommonModes, NSSize, Obj, Sel};
use crate::prefs::Prefs;
use crate::store::SensorStore;
use crate::update::Update;

const TAG_QUIT: isize = -22;
const TAG_LOGIN: isize = -23;
const TAG_UPDATE: isize = -24;
const TAG_FIXED_WIDTH: isize = -25;
const TAG_INTERVAL_BASE: isize = -30;

/// A check that has not answered in this long is treated as failed. curl is
/// bounded, so this only catches a wedged worker, and it is what keeps the row
/// from sitting on "Checking for Updates…" forever.
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// The same for an install, which has a 100 MB download and two mounts to do.
const INSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);
/// How long the row keeps saying "up to date" after a check found nothing.
const CURRENT_FOR: std::time::Duration = std::time::Duration::from_secs(600);

/// How long the row that collects a poll keeps looking for it. The first poll
/// of a session opens the driver and enumerates its keys, which takes longer
/// than one wait; a result nobody collects until the next refresh tick leaves
/// the menu showing the numbers it had before the app restarted.
const COLLECT_RETRIES: u8 = 20;

struct AppState {
    store: SensorStore,
    prefs: Prefs,
    controller: Obj,
    status_item: Obj,
    /// Tag -> menu item, so open menus can be updated in place.
    live_items: HashMap<isize, Obj>,
    open_menu: Obj,
    last_signature: String,
    /// Value column of the dropdown, fixed while the menu is open so a value
    /// that grows between ticks moves nothing.
    row_tab: f64,
    /// Same, per category, for each submenu.
    sensor_tabs: [f64; Category::COUNT],
    /// Submenu -> category, so a submenu can be filled when it is about to
    /// open instead of all nine being built with the dropdown.
    submenus: HashMap<usize, Category>,
    /// The category whose submenu is on screen, if any.
    open_submenu: Option<Category>,
    /// Reads sensors off the main thread; only the GUI runs one.
    poller: Option<crate::poller::Poller>,
    /// Fires once per refresh interval.
    tick_timer: Obj,
    /// One-shot timer that picks up the snapshot a tick asked for.
    collect_timer: Obj,
    /// How many times that timer has looked again for a poll still running,
    /// so a slow first poll is collected early and a wedged one is not chased
    /// for ever.
    collect_waits: u8,
    /// What the menu bar currently says, so an unchanged reading is not
    /// redrawn.
    last_title: String,
    /// The Start at Login row, so its checkmark can follow the toggle.
    login_item: Obj,
    /// Tag -> sensor id, so a click selects the sensor it names even if the
    /// list behind the menu was rebuilt with a different order.
    live_ids: HashMap<isize, String>,
    /// Tag -> the value text already drawn in that row, so a refresh only
    /// rebuilds the rows whose reading actually changed.
    row_values: HashMap<isize, String>,
    /// How many times the poller thread has been replaced after dying.
    poller_restarts: u8,
    /// Width remembered for each slot on the bar, at its widest so far, with
    /// the text it was measured from.
    status_slots: HashMap<Category, (String, f64)>,
    /// Width of the two spaces between slots, measured once.
    status_separator: f64,
    /// The length currently set on the status item.
    status_length: f64,
    /// The Check for Updates row, so its label can change the moment an answer
    /// lands instead of waiting for the next rebuild.
    update_item: Obj,
    /// Reopens the dropdown after a row was chosen, so a toggle does not shut
    /// the menu on every click.
    reopen_timer: Obj,
    /// The Check for Updates row and the pipe a worker thread answers down.
    updates: Updates,
}

/// The update row's state, and the pipe a worker thread hands its answer down.
///
/// `update::check` and `update::install` block on curl and on hdiutil for as
/// long as the network and the disk take, and the app's state is a
/// thread-local only the main thread may touch. So the work runs on a plain
/// thread that owns nothing but the slot it writes into, and a short timer on
/// the main run loop picks the answer up from there.
struct Updates {
    status: UpdateStatus,
    /// The release the row offers, once a check found one.
    found: Option<Update>,
    /// True from the moment a worker starts until its answer is applied, so a
    /// second click cannot start a second one.
    busy: bool,
    /// Whether the check in flight was asked for by a click. The quiet check
    /// at launch says nothing when it finds nothing, so the row keeps
    /// offering a check instead of claiming the app is current forever.
    announced: bool,
    /// When a check finished and found nothing: the row says so until then,
    /// and offers a check again after it. A result belongs to the minutes it
    /// was made in.
    current_until: Option<Instant>,
    /// When the work in flight stops being believable; the watchdog in
    /// `update_poll` clears the row when a worker never answers.
    deadline: Option<Instant>,
    /// Written by the worker, read by the main thread, and nothing else crosses.
    outcome: Arc<Mutex<Option<Outcome>>>,
    /// Polls `outcome` while a worker runs; invalidated as soon as it lands.
    timer: Obj,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateStatus {
    /// Nothing in flight. With a release in `found`, the row offers to install
    /// it — that is what a check that found something leaves behind, so a
    /// silent check at launch needs no state of its own.
    Idle,
    Checking,
    Current,
    Installing,
    Failed,
}

/// What a worker thread hands back to the main thread.
enum Outcome {
    /// A check ran: the release it found, or None when this build is newest.
    Checked(Option<Update>),
    /// The new copy has replaced the installed one and is already running.
    Installed,
    /// Whatever ran failed, with the words the app would print.
    Failed(String),
}

impl Updates {
    /// No check has run yet: the state the app starts in and the headless
    /// dumps build their menu from.
    fn idle() -> Updates {
        Updates {
            status: UpdateStatus::Idle,
            found: None,
            busy: false,
            announced: false,
            current_until: None,
            deadline: None,
            outcome: Arc::new(Mutex::new(None)),
            timer: std::ptr::null_mut(),
        }
    }

    /// The row's label and whether a click on it does anything.
    ///
    /// The dropdown is rebuilt from this every time it opens, so a result that
    /// arrived while the menu was closed is what the next open shows.
    fn row(&self) -> (String, bool) {
        let status = self.effective_status();
        let label = match (&status, &self.found) {
            (UpdateStatus::Idle, Some(update)) => format!("Update to v{}", update.version),
            // The quiet check at launch leaves the label alone; a click while
            // it is out only makes it announce itself.
            (UpdateStatus::Idle, None) => "Check for Updates".to_string(),
            (UpdateStatus::Checking, _) => "Checking for Updates…".to_string(),
            (UpdateStatus::Current, _) => format!(
                "Vitals is up to date · v{}",
                crate::update::current_version()
            ),
            (UpdateStatus::Installing, _) => "Updating…".to_string(),
            (UpdateStatus::Failed, _) => "Update check failed — try again".to_string(),
        };
        // Only the two states with work of their own in flight refuse a click;
        // up to date and failed both mean "check again", which is harmless.
        let enabled = !matches!(
            status,
            UpdateStatus::Checking | UpdateStatus::Installing
        );
        (label, enabled)
    }

    /// The state the row shows, with an expired "up to date" read as idle.
    fn effective_status(&self) -> UpdateStatus {
        match (self.status, self.current_until) {
            (UpdateStatus::Current, Some(until)) if Instant::now() >= until => UpdateStatus::Idle,
            (status, _) => status,
        }
    }

    /// One line naming the state, for the headless menu dump.
    fn summary(&self) -> String {
        let status = self.effective_status();
        match (&status, &self.found) {
            (UpdateStatus::Idle, Some(update)) => format!("update available: v{}", update.version),
            (UpdateStatus::Idle, None) => "idle".to_string(),
            (UpdateStatus::Checking, _) => "checking".to_string(),
            (UpdateStatus::Current, _) => {
                format!("up to date: v{}", crate::update::current_version())
            }
            (UpdateStatus::Installing, _) => "installing".to_string(),
            (UpdateStatus::Failed, _) => "failed".to_string(),
        }
    }

    /// What a click on the row does, read from the same state `row` shows.
    fn activate(&mut self, controller: Obj) {
        match self.effective_status() {
            UpdateStatus::Idle if self.found.is_some() => self.begin_install(controller),
            UpdateStatus::Idle | UpdateStatus::Current | UpdateStatus::Failed => {
                self.begin_check(controller, true)
            }
            UpdateStatus::Checking | UpdateStatus::Installing => {}
        }
    }

    /// Asks GitHub for the newest release. `announce` puts "Checking for
    /// Updates…" on the row; the launch check leaves it quiet.
    fn begin_check(&mut self, controller: Obj, announce: bool) {
        if self.busy {
            // A check is already out. A click that asked for one shows it as
            // running, since that is the truth the user asked to see.
            if announce {
                self.status = UpdateStatus::Checking;
            }
            return;
        }
        self.busy = true;
        self.found = None;
        self.current_until = None;
        self.deadline = Some(Instant::now() + CHECK_TIMEOUT);
        self.announced = announce;
        if announce {
            self.status = UpdateStatus::Checking;
        }
        self.spawn(controller, || match crate::update::check() {
            Ok(Some(update)) => Outcome::Checked(Some(update)),
            Ok(None) => Outcome::Checked(None),
            Err(error) => Outcome::Failed(error),
        });
    }

    /// Replaces the installed app with the release the row offers.
    fn begin_install(&mut self, controller: Obj) {
        if self.busy {
            return;
        }
        let Some(update) = self.found.take() else { return };
        self.busy = true;
        self.deadline = Some(Instant::now() + INSTALL_TIMEOUT);
        self.status = UpdateStatus::Installing;
        self.spawn(controller, move || match crate::update::install(&update) {
            Ok(()) => Outcome::Installed,
            Err(error) => Outcome::Failed(error),
        });
    }

    /// Runs `work` on a thread of its own and starts watching for its answer.
    fn spawn(&mut self, controller: Obj, work: impl FnOnce() -> Outcome + Send + 'static) {
        let slot = Arc::clone(&self.outcome);
        std::thread::spawn(move || {
            // A panicking worker would otherwise leave the row saying
            // "Checking…" until the app is restarted, so the panic becomes an
            // answer like any other.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Outcome::Failed("the update worker stopped".to_string()));
            // The slot holds one answer and this thread is its only writer, so
            // a poisoned lock still carries everything the main thread needs.
            *slot.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(outcome);
        });
        self.schedule_poll(controller);
    }

    /// Arms the timer that watches for the worker's answer: short, because the
    /// answer is only a lock away, and cancelled the moment it is applied.
    fn schedule_poll(&mut self, controller: Obj) {
        unsafe {
            if !self.timer.is_null() {
                objc::send_void(self.timer, "invalidate");
                objc::release(self.timer);
                self.timer = std::ptr::null_mut();
            }
            let timer = objc::send_timer(
                objc::class("NSTimer"),
                "timerWithTimeInterval:target:selector:userInfo:repeats:",
                0.25,
                controller,
                objc::selector("updatePoll:"),
                std::ptr::null_mut(),
                true,
            );
            arm_timer(timer);
            self.timer = objc::retain(timer);
        }
    }

    /// Fails the work in flight when its worker has gone quiet, so the row can
    /// never sit on "Checking for Updates…" for good.
    fn timed_out(&mut self) -> bool {
        if !self.busy {
            return false;
        }
        let Some(deadline) = self.deadline else {
            return false;
        };
        if Instant::now() < deadline {
            return false;
        }
        // The worker keeps its own slot, so a late answer lands nowhere
        // instead of overwriting the state a later check is using.
        self.outcome = Arc::new(Mutex::new(None));
        self.busy = false;
        self.deadline = None;
        self.status = UpdateStatus::Failed;
        unsafe {
            if !self.timer.is_null() {
                objc::send_void(self.timer, "invalidate");
                objc::release(self.timer);
                self.timer = std::ptr::null_mut();
            }
        }
        eprintln!("update: the worker did not answer in time");
        true
    }

    /// Applies an answer. True means a new copy of the app is already running,
    /// so this instance has to terminate.
    fn finish(&mut self, outcome: Outcome) -> bool {
        unsafe {
            if !self.timer.is_null() {
                objc::send_void(self.timer, "invalidate");
                objc::release(self.timer);
                self.timer = std::ptr::null_mut();
            }
        }
        self.busy = false;
        self.deadline = None;
        match outcome {
            Outcome::Checked(Some(update)) => {
                self.found = Some(update);
                self.status = UpdateStatus::Idle;
            }
            Outcome::Checked(None) => {
                // Only a check the user asked for reports the result; the one
                // at launch leaves the row alone.
                self.status = if self.announced {
                    self.current_until = Some(Instant::now() + CURRENT_FOR);
                    UpdateStatus::Current
                } else {
                    UpdateStatus::Idle
                };
            }
            Outcome::Installed => return true,
            Outcome::Failed(error) => {
                // The row's label has to stay one line, so the reason goes
                // where every other failure in this app goes.
                eprintln!("update: {error}");
                self.status = UpdateStatus::Failed;
            }
        }
        false
    }
}

thread_local! {
    // The app is single-threaded: every message, the timer and the menu run on
    // the main run loop. A RefCell makes a re-entrant call panic with the place
    // it happened instead of deadlocking on a lock this thread already holds.
    static STATE: std::cell::RefCell<Option<AppState>> = const { std::cell::RefCell::new(None) };
}

fn with_state<R>(body: impl FnOnce(&mut AppState) -> R) -> R {
    STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let state = borrow.as_mut().expect("state is installed before any message");
        body(state)
    })
}

fn install(state: AppState) {
    STATE.with(|cell| *cell.borrow_mut() = Some(state));
}

pub fn prepare_application() {
    unsafe {
        let _app = objc::send(objc::class("NSApplication"), "sharedApplication");
        crate::icons::load();
    }
}

pub fn run() {
    prepare_application();
    unsafe {
        let app = objc::send(objc::class("NSApplication"), "sharedApplication");
        // LSUIElement is in Info.plist; accessory keeps it true from a terminal.
        objc::send_void_int(app, "setActivationPolicy:", 1);
        let controller = make_controller();
        // The poller owns the SMC connection; opening a second one here would
        // only block the launch for a reading that arrives a moment later.
        let store = SensorStore::for_ui();
        let prefs = Prefs::load();

        install(AppState {
            store,
            prefs,
            controller,
            status_item: std::ptr::null_mut(),
            live_items: HashMap::new(),
            open_menu: std::ptr::null_mut(),
            row_tab: 0.0,
            sensor_tabs: [0.0; Category::COUNT],
            submenus: HashMap::new(),
            open_submenu: None,
            poller: Some(crate::poller::Poller::start()),
            tick_timer: std::ptr::null_mut(),
            collect_timer: std::ptr::null_mut(),
            collect_waits: 0,
            last_title: String::new(),
            login_item: std::ptr::null_mut(),
            last_signature: String::new(),
            live_ids: HashMap::new(),
            row_values: HashMap::new(),
            poller_restarts: 0,
            status_slots: HashMap::new(),
            status_separator: 0.0,
            status_length: 0.0,
            update_item: std::ptr::null_mut(),
            reopen_timer: std::ptr::null_mut(),
            updates: Updates::idle(),
        });

        // The slot is born in -applicationDidFinishLaunching:, not here: AppKit
        // drops a status item created before the app has finished launching,
        // which made the icon come and go between runs.
        objc::send_void_obj(app, "setDelegate:", controller);
        let run_loop = objc::send(objc::class("NSRunLoop"), "mainRunLoop");

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

unsafe extern "C" fn application_did_finish_launching(_this: Obj, _cmd: Sel, _note: Obj) {
    with_state(|state| create_status_item(state));
    with_state(|state| {
        if let Some(poller) = state.poller.as_ref() {
            poller.request(true, &[]);
            schedule_collect(state);
        }
    });
    with_state(schedule_ticks);
    // Once, and quietly: a newer release is on the row by the time the menu is
    // first opened, while a check that fails here only prints its reason, since
    // nobody asked for it and the row still offers a fresh check.
    with_state(|state| {
        let controller = state.controller;
        state.updates.begin_check(controller, false);
    });
}

unsafe extern "C" fn tick_imp(_this: Obj, _cmd: Sel, _timer: Obj) {
    with_state(|state| tick(state));
}

/// Collects what an update worker thread left behind.
///
/// This is the only place a check or an install crosses back into the main
/// thread: the worker wrote one `Outcome` into a slot it shares with nobody
/// else, and everything that touches the app state — and AppKit with it —
/// happens here, on the run loop.
/// Rewrites the visible Check for Updates row from the state machine, so a
/// result shows up in the open menu instead of only on the next rebuild. This
/// is what made a fast check look like a slow one: the worker had answered,
/// and the row still said "Checking…" because nothing repainted it.
fn refresh_update_row(state: &mut AppState) {
    if state.update_item.is_null() {
        return;
    }
    let (title, enabled) = state.updates.row();
    unsafe {
        objc::send_void_obj(state.update_item, "setTitle:", objc::nsstring(&title));
        objc::send_void_int(state.update_item, "setEnabled:", if enabled { 1 } else { 0 });
    }
}

unsafe extern "C" fn update_poll(_this: Obj, _cmd: Sel, _timer: Obj) {
    let replaced = with_state(|state| {
        let outcome = state
            .updates
            .outcome
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        let replaced = match outcome {
            Some(outcome) => state.updates.finish(outcome),
            None => {
                state.updates.timed_out();
                false
            }
        };
        refresh_update_row(state);
        replaced
    });
    if replaced {
        // `update::install` already started the new copy, so leaving this one
        // running would put two icons in the menu bar.
        unsafe {
            let app = objc::send(objc::class("NSApplication"), "sharedApplication");
            objc::send_void_obj(app, "terminate:", std::ptr::null_mut());
        }
    }
}

unsafe extern "C" fn menu_needs_update(_this: Obj, _cmd: Sel, menu: Obj) {
    with_state(|state| match state.submenus.get(&(menu as usize)).copied() {
        Some(category) => {
            let reading = state.store.get(category).clone();
            populate_submenu(state, menu, category, &reading);
        }
        None => populate(state, menu),
    });
}

unsafe extern "C" fn menu_will_open(_this: Obj, _cmd: Sel, menu: Obj) {
    with_state(|state| {
        if let Some(category) = state.submenus.get(&(menu as usize)).copied() {
            // A submenu opening inside the dropdown: the dropdown is still the
            // open menu, and its poll already covers this.
            state.open_submenu = Some(category);
            return;
        }
        state.open_menu = menu;
        state.open_submenu = None;
        // Fresh data the moment the menu opens; from here the refresh tick,
        // which runs in the tracking mode too, is what keeps it live.
        if let Some(poller) = state.poller.as_ref() {
            poller.request(true, &[]);
            schedule_collect(state);
        }
    });
}

unsafe extern "C" fn menu_did_close(_this: Obj, _cmd: Sel, menu: Obj) {
    with_state(|state| {
        if state.submenus.contains_key(&(menu as usize)) {
            // A submenu closed while the dropdown stayed open.
            state.open_submenu = None;
            return;
        }
        state.open_menu = std::ptr::null_mut();
        state.open_submenu = None;
        release_tracked(state, |_| true);
        state.live_ids.clear();
        state.row_values.clear();
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
        let last = ALL_CATEGORIES[Category::COUNT - 1];
        dispatch(state, category_tag(last, None));
        dispatch(state, category_tag(last, None));
        eprintln!("demo: changing the interval");
        dispatch(state, TAG_INTERVAL_BASE - 5);
        eprintln!("demo: actions done");
    });
}

// `cancelTracking` and `performClick:` come straight back into this
// controller (`menuDidClose:`, `menuWillOpen:`), so the state must be released
// before they are sent: holding a borrow across them would re-enter it.
unsafe extern "C" fn demo_close(_this: Obj, _cmd: Sel, _timer: Obj) {
    let menu = with_state(|state| state.open_menu);
    if !menu.is_null() {
        objc::send_void(menu, "cancelTracking");
    }
}

unsafe extern "C" fn demo_open(_this: Obj, _cmd: Sel, _timer: Obj) {
    let item = with_state(|state| state.status_item);
    if item.is_null() {
        return;
    }
    if let Some(button) = button_of(item) {
        objc::send_void_obj(button, "performClick:", std::ptr::null_mut());
    }
}

unsafe extern "C" fn handle_action(_this: Obj, _cmd: Sel, sender: Obj) {
    let tag = with_state(|_| unsafe { objc::send_int_result(sender, "tag") });
    with_state(|state| {
        dispatch(state, tag);
        // Choosing a row closes an NSMenu; the toggles here are meant to be
        // flipped a few times in a row, so the dropdown is put back as soon as
        // the click has been handled. Quit is the one row that means it.
        if tag != TAG_QUIT {
            schedule_reopen(state);
        }
    });
}

/// Reopens the dropdown a moment from now.
///
/// Reopening from inside the action runs a tracking loop inside the one that
/// is still unwinding, so it is deferred by a short timer instead.
fn schedule_reopen(state: &mut AppState) {
    unsafe {
        if !state.reopen_timer.is_null() {
            objc::send_void(state.reopen_timer, "invalidate");
            objc::release(state.reopen_timer);
            state.reopen_timer = std::ptr::null_mut();
        }
        let timer = objc::send_timer(
            objc::class("NSTimer"),
            "timerWithTimeInterval:target:selector:userInfo:repeats:",
            0.05,
            state.controller,
            objc::selector("reopenMenu:"),
            std::ptr::null_mut(),
            false,
        );
        arm_timer(timer);
        state.reopen_timer = objc::retain(timer);
    }
}

/// Puts the dropdown back under the status item, after releasing the state:
/// the menu tracks until the next choice, and everything it calls back into
/// needs the borrow.
unsafe extern "C" fn reopen_menu(_this: Obj, _cmd: Sel, _timer: Obj) {
    let (item, menu) = with_state(|state| {
        unsafe {
            if !state.reopen_timer.is_null() {
                objc::release(state.reopen_timer);
                state.reopen_timer = std::ptr::null_mut();
            }
        }
        if state.status_item.is_null() {
            return (std::ptr::null_mut(), std::ptr::null_mut());
        }
        let item = state.status_item;
        (item, unsafe { objc::send(item, "menu") })
    });
    if item.is_null() || menu.is_null() {
        return;
    }
    let Some(button) = button_of(item) else { return };
    let height = unsafe { objc::send_rect(button, "bounds").height };
    unsafe {
        objc::send_void_item_point_view(
            menu,
            "popUpMenuPositioningItem:atLocation:inView:",
            std::ptr::null_mut(),
            0.0,
            height,
            button,
        );
    }
}

fn make_controller() -> Obj {
    unsafe {
        let name = CString::new("VitalsController").unwrap();
        let cls = objc::objc_allocateClassPair(objc::class("NSObject"), name.as_ptr(), 0);
        let void_obj = CString::new("v@:@").unwrap();

        let methods: [(&str, *const ()); 12] = [
            ("applicationDidFinishLaunching:", application_did_finish_launching as *const ()),
            ("tick:", tick_imp as *const ()),
            ("demoOpen:", demo_open as *const ()),
            ("demoClose:", demo_close as *const ()),
            ("demoActions:", demo_actions as *const ()),
            ("collect:", collect as *const ()),
            ("reopenMenu:", reopen_menu as *const ()),
            ("updatePoll:", update_poll as *const ()),
            ("menuNeedsUpdate:", menu_needs_update as *const ()),
            ("menuWillOpen:", menu_will_open as *const ()),
            ("menuDidClose:", menu_did_close as *const ()),
            ("handleAction:", handle_action as *const ()),
        ];
        assert!(!cls.is_null(), "VitalsController is registered twice");
        for (selector_name, implementation) in methods {
            let sel = objc::selector(selector_name);
            let types = void_obj.as_ptr();
            let added = objc::class_addMethod(cls, sel, implementation as *const std::ffi::c_void, types);
            // Every method here is `- (void)name:(id)sender`, so one type
            // string covers them; a silent miss would leave the app with a
            // delegate that answers nothing.
            assert!(added, "{selector_name} could not be installed");
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

/// Creates the single status item and its menu. There is one slot by design,
/// so nothing here needs rebuilding; only the title changes.
fn create_status_item(state: &mut AppState) {
    unsafe {
        let bar = objc::send(objc::class("NSStatusBar"), "systemStatusBar");
        // +statusItemWithLength: hands back an autoreleased item, and the
        // pointer we keep has to outlive the pool that created it. It is born
        // variable and pinned by the first title render, which is where the
        // width is measured from the readings on the bar.
        let item = objc::retain(objc::send_float(bar, "statusItemWithLength:", -1.0));
        let menu = make_menu(state.controller);
        objc::send_void_obj(item, "setMenu:", menu);
        objc::release(menu);
        state.status_item = item;
        state.last_signature = signature(&state.prefs);
        state.last_title.clear();
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

/// What the menu bar slot says: one `key=value;` per shown category.
///
/// An empty bar still gets a title, because a status item with neither title
/// nor image collapses to zero width — the menu would then be unreachable and
/// the only way back would be editing the preference domain by hand.
pub fn status_title(prefs: &Prefs, store: &SensorStore) -> String {
    let categories = prefs.ordered_menu_bar();
    if categories.is_empty() {
        return "Vitals".to_string();
    }
    categories
        .iter()
        .map(|category| {
            let reading = store.get(*category);
            let value = reading.reading_for(prefs.selection(*category).map(|s| s.as_str()));
            format!("{}={};", category.key(), value.compact)
        })
        .collect()
}

/// What one slot of the menu bar shows: its reading as the slot prints it, or
/// a dash when there is none.
fn status_slot_text(reading: &crate::model::Reading) -> String {
    if reading.has_data() {
        reading.compact.clone()
    } else {
        "—".to_string()
    }
}

/// Appends `text` in `font`, one run of an attributed string.
unsafe fn append_run(string: Obj, text: &str, font: Obj) {
    let attributes = objc::send(objc::class("NSMutableDictionary"), "dictionary");
    objc::send_void_obj_obj(attributes, "setObject:forKey:", font, objc::nsstring("NSFont"));
    let piece = objc::send_obj_obj(
        objc::alloc("NSAttributedString"),
        "initWithString:attributes:",
        objc::nsstring(text),
        attributes,
    );
    objc::send_void_obj(string, "appendAttributedString:", piece);
    objc::release(piece);
}

unsafe fn status_value_font() -> Obj {
    objc::send_obj_two_floats(
        objc::class("NSFont"),
        "monospacedDigitSystemFontOfSize:weight:",
        objc::system_font_size(),
        0.0,
    )
}

/// Width of one slot: its icon glyph, a thin space, then the value, in the
/// fonts the title is drawn with.
unsafe fn slot_width(category: Option<Category>, value: &str) -> f64 {
    let _pool = objc::AutoreleasePool::new();
    let font = status_value_font();
    let run = objc::send_obj(
        objc::alloc("NSMutableAttributedString"),
        "initWithString:",
        objc::nsstring(""),
    );
    if let Some(category) = category {
        if let Some(icon_font) = crate::icons::font(STATUS_ICON_SIZE) {
            append_run(run, &category.icon().to_string(), icon_font);
        }
    }
    append_run(run, &format!("\u{2009}{value}"), font);
    let width = objc::send_size(run, "size").width;
    objc::release(run);
    width.ceil()
}

/// Sizes the status item to what is on the bar, and does it on the bar's
/// terms rather than the widgets: each slot is measured from the reading it is
/// actually showing, so a bar with one category stays as narrow as one
/// category.
///
/// Nothing moves while a value changes shape, because the width remembered for
/// a slot only ever grows: "9°" becoming "100°" widens the widget once, and
/// the next refresh that reads "43°" does not shrink it back and shift the
/// whole item sideways. Shrinking happens only when a category leaves the bar,
/// which is when the length is recomputed from the categories still there.
fn status_slots(state: &AppState) -> Vec<(Category, String)> {
    state
        .prefs
        .ordered_menu_bar()
        .into_iter()
        .map(|category| {
            let reading = state.store.get(category);
            let value = reading.reading_for(state.prefs.selection(category).map(|s| s.as_str()));
            (category, status_slot_text(value))
        })
        .collect()
}

/// The length these slots measure, with nothing remembered in between.
fn measured_status_length(slots: &[(Category, String)]) -> f64 {
    let _pool = objc::AutoreleasePool::new();
    unsafe {
        let mut total = STATUS_SLOT_PADDING;
        if slots.is_empty() {
            total += slot_width(None, "Vitals");
        }
        for (index, (category, value)) in slots.iter().enumerate() {
            if index > 0 {
                total += slot_width(None, "  ");
            }
            total += slot_width(Some(*category), value);
        }
        total.ceil()
    }
}

fn apply_status_length(state: &mut AppState, slots: &[(Category, String)]) {
    if state.status_item.is_null() {
        return;
    }
    if !state.prefs.fixed_status_width {
        // Off: the item sizes itself to whatever the readings say, which is
        // the AppKit default and what the row toggles back to.
        if state.status_length > 0.0 {
            state.status_length = 0.0;
            unsafe {
                objc::send_void_float(state.status_item, "setLength:", -1.0);
            }
        }
        return;
    }
    let total = unsafe {
        let separator = if state.status_separator > 0.0 {
            state.status_separator
        } else {
            let measured = slot_width(None, "  ");
            state.status_separator = measured;
            measured
        };
        let mut total = STATUS_SLOT_PADDING;
        if slots.is_empty() {
            // The empty bar still says this, and that title is the only way
            // back to the menu.
            total += slot_width(None, "Vitals");
        }
        for (index, (category, value)) in slots.iter().enumerate() {
            if index > 0 {
                total += separator;
            }
            let width = match state.status_slots.get(category) {
                Some((text, width)) if text == value => *width,
                Some((_, width)) => slot_width(Some(*category), value).max(*width),
                None => slot_width(Some(*category), value),
            };
            state.status_slots.insert(*category, (value.clone(), width));
            total += width;
        }
        total.ceil()
    };
    if (total - state.status_length).abs() < 0.5 {
        return;
    }
    state.status_length = total;
    unsafe {
        objc::send_void_float(state.status_item, "setLength:", total);
    }
}

fn update_status_titles(state: &mut AppState) {
    unsafe {
        let categories = state.prefs.ordered_menu_bar();
        let stated = status_title(&state.prefs, &state.store);
        if stated == state.last_title && !state.status_item.is_null() {
            return;
        }
        {
            let item = &state.status_item;
            if item.is_null() {
                return;
            }
            let Some(button) = button_of(*item) else { return };
            // Sized from the readings about to be drawn, so the item is never
            // narrower than its content and never wider than it needs.
            let slots = status_slots(state);
            apply_status_length(state, &slots);
            let mut title = Attributed::new();
            for (index, (category, value)) in slots.iter().enumerate() {
                if index > 0 {
                    title.append_string("  ");
                }
                title.append_symbol(*category);
                title.append_string(&format!("\u{2009}{value}"));
            }
            if title.is_empty() {
                title.append_string("Vitals");
            }
            let finished = title.finish();
            objc::send_void_obj(button, "setAttributedTitle:", finished);
            objc::release(finished);
            // Committed after the draw: a call that found no button must not
            // leave the cache claiming a title the item never got.
            state.last_title = stated;
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

/// Drops the app's own reference to the rows whose tags `should_drop` accepts.
/// The menu holds a reference of its own, so this never destroys a row still
/// on screen; it only gives back the one this app took when it built the row,
/// which is what a rebuild has to do or every rebuild keeps a live row forever.
fn release_tracked(state: &mut AppState, should_drop: impl Fn(isize) -> bool) {
    let stale: Vec<isize> = state
        .live_items
        .keys()
        .copied()
        .filter(|tag| should_drop(*tag))
        .collect();
    unsafe {
        for tag in stale {
            if let Some(item) = state.live_items.remove(&tag) {
                objc::release(item);
            }
        }
    }
}

// MARK: - Tick

/// What a poll of the menu bar has to cover.
fn shown_categories(state: &AppState) -> Vec<(Category, Option<String>)> {
    state
        .prefs
        .ordered_menu_bar()
        .into_iter()
        .map(|category| (category, state.prefs.selection(category).cloned()))
        .collect()
}

fn tick(state: &mut AppState) {
    // The poll runs on the worker thread, so this only asks for one and then
    // comes back for the result. Nothing is read on the main thread.
    match state.poller.as_ref() {
        Some(poller) => {
            let full = !state.open_menu.is_null();
            poller.request(full, &shown_categories(state));
            schedule_collect(state);
        }
        None => {
            state.store.refresh();
            show_latest(state, true);
        }
    }
}

/// Picks up a snapshot a tick asked for.
///
/// It applies whatever arrived and arms nothing of its own: a snapshot that is
/// not ready yet is simply collected by the next refresh tick, so the menu
/// updates on the interval the user configured whether it is open or closed.
unsafe extern "C" fn collect(_this: Obj, _cmd: Sel, _timer: Obj) {
    with_state(|state| {
        unsafe {
            if !state.collect_timer.is_null() {
                objc::release(state.collect_timer);
                state.collect_timer = std::ptr::null_mut();
            }
        }
        let fresh = match state.poller.as_ref().map(|poller| poller.take()) {
            Some(Polled::Snapshot(snapshot)) => {
                state.collect_waits = 0;
                state.store.apply(snapshot);
                true
            }
            Some(Polled::Dead) => {
                // The worker died: without this the menu keeps presenting its
                // last reading as if it were live.
                restart_poller(state);
                look_again(state);
                false
            }
            Some(Polled::Empty) | None => {
                look_again(state);
                false
            }
        };
        show_latest(state, fresh);
    });
}

/// Arms one more short wait for a poll that has not finished. Without it the
/// result of a slow poll — the first of a session opens the driver and
/// enumerates every key — would sit in the channel until the next refresh
/// tick, and the menu would show the numbers from before the restart.
fn look_again(state: &mut AppState) {
    if state.collect_waits >= COLLECT_RETRIES {
        return;
    }
    state.collect_waits = state.collect_waits.saturating_add(1);
    schedule_collect(state);
}

/// Replaces a poller whose thread is gone, a bounded number of times: a
/// deterministic panic would otherwise spin up a thread on every fire.
fn restart_poller(state: &mut AppState) {
    state.store = SensorStore::for_ui();
    if state.poller_restarts >= 3 {
        state.poller = None;
        return;
    }
    state.poller_restarts += 1;
    state.poller = Some(crate::poller::Poller::start());
    if let Some(poller) = state.poller.as_ref() {
        poller.request(true, &[]);
    }
}

fn show_latest(state: &mut AppState, fresh: bool) {
    let signature = signature(&state.prefs);
    if signature != state.last_signature {
        state.last_signature = signature;
        state.last_title.clear();
    }
    update_status_titles(state);
    // Only rows that show a changed reading are worth rebuilding, and they are
    // rebuilt once per refresh interval, on the same tick as the menu bar.
    if fresh && !state.open_menu.is_null() {
        refresh_values(state);
    }
}

/// Adds `timer` to the main run loop for both common modes and event tracking.
///
/// A menu tracks in NSEventTrackingRunLoopMode, where a timer registered for
/// the common modes alone does not run. Being in both is what lets one cadence
/// serve an open menu and a closed one: the refresh tick keeps firing while
/// the dropdown is held open, so the menu redraws on the interval the user
/// picked instead of on a timer of its own.
unsafe fn arm_timer(timer: Obj) {
    let run_loop = objc::send(objc::class("NSRunLoop"), "mainRunLoop");
    objc::send_void_obj_obj(run_loop, "addTimer:forMode:", timer, NSRunLoopCommonModes);
    objc::send_void_obj_obj(
        run_loop,
        "addTimer:forMode:",
        timer,
        objc::NSEventTrackingRunLoopMode,
    );
}

/// Arms the one-shot timer that collects the poll requested a moment ago.
fn schedule_collect(state: &mut AppState) {
    unsafe {
        if !state.collect_timer.is_null() {
            objc::send_void(state.collect_timer, "invalidate");
            objc::release(state.collect_timer);
            state.collect_timer = std::ptr::null_mut();
        }
        let timer = objc::send_timer(
            objc::class("NSTimer"),
            "timerWithTimeInterval:target:selector:userInfo:repeats:",
            0.4,
            state.controller,
            objc::selector("collect:"),
            std::ptr::null_mut(),
            false,
        );
        arm_timer(timer);
        state.collect_timer = objc::retain(timer);
    }
}

/// One timer per refresh interval, with tolerance so the system can hold the
/// wakeup until it has other work to do.
fn schedule_ticks(state: &mut AppState) {
    unsafe {
        if !state.tick_timer.is_null() {
            objc::send_void(state.tick_timer, "invalidate");
            objc::release(state.tick_timer);
            state.tick_timer = std::ptr::null_mut();
        }
        let interval = state.prefs.interval;
        let timer = objc::send_timer(
            objc::class("NSTimer"),
            "timerWithTimeInterval:target:selector:userInfo:repeats:",
            interval,
            state.controller,
            objc::selector("tick:"),
            std::ptr::null_mut(),
            true,
        );
        objc::send_void_float(timer, "setTolerance:", (interval * 0.25).max(0.2));
        arm_timer(timer);
        state.tick_timer = objc::retain(timer);
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
    set_row_title_with_symbol(item, None, left, right, tab_stop, dim);
}

unsafe fn set_row_title_with_symbol(
    item: Obj,
    symbol: Option<char>,
    left: &str,
    right: &str,
    tab_stop: f64,
    dim: bool,
) {
    let attributed = Attributed::row_with_symbol(symbol, left, right, tab_stop, dim);
    objc::send_void_obj(item, "setAttributedTitle:", attributed);
    // The item retains it; this drops the +1 we hold from -initWithAttributedString:.
    objc::release(attributed);
}

/// Right edge of the value column. Values are right-aligned here, so a short
/// Right edge of the value column: values are right-aligned here, so a short
/// value sits against the same edge as a long one.
fn column_right_for(labels: &[String], values: &[String]) -> f64 {
    column_right_with(labels, values, 0.0)
}

fn column_right_with(labels: &[String], values: &[String], extra: f64) -> f64 {
    let widest_label = widest_text(labels, false);
    let widest_value = widest_text(values, true);
    (widest_label.ceil() + extra + COLUMN_GUTTER + widest_value.ceil()).ceil()
}

const COLUMN_GUTTER: f64 = 22.0;
/// Square an icon is drawn into.
const ICON_BOX: f64 = 14.0;
/// Icon size in the menu bar, where the text is 13 pt.
const STATUS_ICON_SIZE: f64 = 13.0;
/// Room the bar button leaves either side of its title, so a slot measured to
/// the text alone would clip it.
const STATUS_SLOT_PADDING: f64 = 10.0;
/// Where every category name starts when the row has an icon.
const ICON_SLOT: f64 = 22.0;


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
    let _pool = objc::AutoreleasePool::new();
    unsafe {
        objc::send_void(menu, "removeAllItems");
        release_tracked(state, |_| true);
        state.live_ids.clear();
        state.row_values.clear();
        // The submenus are rebuilt with the dropdown, so the mapping from the
        // previous open's objects is stale from here on.
        state.submenus.clear();

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
        state.row_tab = column_right_with(&labels, &values, ICON_SLOT);
        let row_tab = state.row_tab;

        for category in ALL_CATEGORIES {
            let reading = state.store.get(category).clone();
            let item = menu_item(category.title(), "", 0);
            let value = reading.reading_for(state.prefs.selection(category).map(|s| s.as_str()));
            set_row_title_with_symbol(
                item,
                Some(category.icon()),
                category.title(),
                if value.has_data() { &value.text } else { "No data" },
                row_tab,
                false,
            );
            let submenu = objc::send_obj(
                objc::alloc("NSMenu"),
                "initWithTitle:",
                objc::nsstring(category.title()),
            );
            objc::send_void_int(submenu, "setAutoenablesItems:", 0);
            // Filling a submenu costs a row per sensor it holds, so it happens
            // when the submenu is opened rather than with the dropdown.
            objc::send_void_obj(submenu, "setDelegate:", state.controller);
            state.submenus.insert(submenu as usize, category);
            objc::send_void_obj(item, "setSubmenu:", submenu);
            objc::release(submenu);
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
            objc::release(item);
        }
        objc::send_void_obj(interval_item, "setSubmenu:", interval_menu);
        objc::release(interval_menu);
        objc::send_void_obj(menu, "addItem:", interval_item);
        objc::release(interval_item);

        let fixed_width = action_item("Fixed Status Width", state.controller, TAG_FIXED_WIDTH);
        objc::send_void_int(
            fixed_width,
            "setState:",
            if state.prefs.fixed_status_width { 1 } else { 0 },
        );
        objc::send_void_obj(menu, "addItem:", fixed_width);
        objc::release(fixed_width);

        let login_item = action_item("Start at Login", state.controller, TAG_LOGIN);
        objc::send_void_int(
            login_item,
            "setState:",
            if state.prefs.start_at_login { 1 } else { 0 },
        );
        objc::send_void_obj(menu, "addItem:", login_item);
        if !state.login_item.is_null() {
            objc::release(state.login_item);
        }
        state.login_item = login_item;


        let (title, enabled) = state.updates.row();
        let update = action_item(&title, state.controller, TAG_UPDATE);
        objc::send_void_int(update, "setEnabled:", if enabled { 1 } else { 0 });
        objc::send_void_obj(menu, "addItem:", update);
        if !state.update_item.is_null() {
            objc::release(state.update_item);
        }
        state.update_item = update;

        objc::send_void_obj(menu, "addItem:", objc::send(objc::class("NSMenuItem"), "separatorItem"));
        let quit = action_item("Quit Vitals", state.controller, TAG_QUIT);
        objc::send_void_obj(quit, "setKeyEquivalent:", objc::nsstring("q"));
        objc::send_void_obj(menu, "addItem:", quit);
        objc::release(quit);
    }
}

fn populate_submenu(state: &mut AppState, menu: Obj, category: Category, reading: &CategoryReading) {
    let _pool = objc::AutoreleasePool::new();
    unsafe {
        objc::send_void(menu, "removeAllItems");
        let first_tag = category_tag(category, None) - 1;
        release_tracked(state, |tag| (first_tag..first_tag + 1000).contains(&tag));
        state
            .live_ids
            .retain(|tag, _| *tag < first_tag || *tag >= first_tag + 1000);
        // A checkmark means "this row is what the menu bar is showing", so a
        // category that is not in the menu bar has no checked row at all.
        let shown = state.prefs.menu_bar.contains(&category);
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
        objc::send_void_int(
            average,
            "setState:",
            if shown && active.is_none() { 1 } else { 0 },
        );
        objc::send_void_obj(menu, "addItem:", average);
        state.live_items.insert(category_tag(category, None), average);

        if reading.sensors.is_empty() {
            objc::send_void_obj(menu, "addItem:", objc::send(objc::class("NSMenuItem"), "separatorItem"));
            let none = menu_item("No data", "", 0);
            objc::send_void_int(none, "setEnabled:", 0);
            objc::send_void_obj(menu, "addItem:", none);
            objc::release(none);
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
            let on = shown && active.as_deref() == Some(sensor.id.as_str());
            objc::send_void_int(item, "setState:", if on { 1 } else { 0 });
            objc::send_void_obj(menu, "addItem:", item);
            state.live_items.insert(tag, item);
            state.live_ids.insert(tag, sensor.id.clone());
        }
    }
}

/// Rewrites the value columns of the menu that is currently open.
///
/// Only rows whose value changed are redrawn: this runs on every refresh tick
/// while a menu is open, and rebuilding all nine rows and their paragraph
/// styles each fire is what made the menu stutter.
fn refresh_values(state: &mut AppState) {
    let _pool = objc::AutoreleasePool::new();
    unsafe {
        let changed: Vec<(Category, String)> = ALL_CATEGORIES
            .iter()
            .copied()
            .map(|category| (category, row_value_text(state, category)))
            .filter(|(category, text)| {
                state.row_values.get(&row_tag(*category)).map(|s| s.as_str()) != Some(text.as_str())
            })
            .collect();

        let submenu = state.open_submenu;
        let submenu_rows: Vec<(isize, String, String)> = match submenu {
            Some(category) => {
                let reading = state.store.get(category);
                let mut rows = vec![(
                    category_tag(category, None),
                    "Average".to_string(),
                    if reading.average.has_data() {
                        reading.average.text.clone()
                    } else {
                        "No data".to_string()
                    },
                )];
                rows.extend(reading.sensors.iter().enumerate().map(|(index, sensor)| {
                    let left = match display_key(sensor) {
                        Some(key) => format!("{key}  {}", sensor.label),
                        None => sensor.label.clone(),
                    };
                    (category_tag(category, Some(index)), left, sensor.reading.text.clone())
                }));
                rows
            }
            None => Vec::new(),
        };
        let submenu_changed: Vec<(isize, String, String)> = submenu_rows
            .into_iter()
            .filter(|(tag, _, text)| {
                state.row_values.get(tag).map(|s| s.as_str()) != Some(text.as_str())
            })
            .collect();

        if changed.is_empty() && submenu_changed.is_empty() {
            return;
        }

        // The first open can land before the first poll, when every value is
        // still a placeholder and the column the menu was built with is too
        // narrow for what arrives a moment later.
        let labels: Vec<String> = ALL_CATEGORIES
            .iter()
            .map(|category| category.title().to_string())
            .collect();
        let values: Vec<String> = ALL_CATEGORIES
            .iter()
            .map(|category| row_value_text(state, *category))
            .collect();
        let needed = column_right_with(&labels, &values, ICON_SLOT);
        if needed > state.row_tab {
            // Rows written in place keep the tab stops they were laid out
            // with, so a column that has to grow shows its values ending
            // short of the right edge until the menu is opened again. Building
            // the menu afresh is exactly what that next open does.
            let menu = state.open_menu;
            if !menu.is_null() && state.open_submenu.is_none() {
                populate(state, menu);
                return;
            }
            state.row_tab = needed;
        }
        let row_tab = state.row_tab;

        for (category, text) in &changed {
            let Some(item) = state.live_items.get(&row_tag(*category)).copied() else {
                continue;
            };
            set_row_title_with_symbol(
                item,
                Some(category.icon()),
                category.title(),
                text,
                row_tab,
                false,
            );
            state.row_values.insert(row_tag(*category), text.clone());
        }

        let Some(category) = submenu else {
            return;
        };
        let sensor_tab = state.sensor_tabs[category.index()];
        for (tag, left, text) in &submenu_changed {
            let Some(item) = state.live_items.get(tag).copied() else {
                continue;
            };
            set_row_title(item, left, text, sensor_tab, true);
            state.row_values.insert(*tag, text.clone());
        }
    }
}

fn row_value_text(state: &AppState, category: Category) -> String {
    state
        .store
        .get(category)
        .describe(state.prefs.selection(category).map(|s| s.as_str()))
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
        // The id the row was built for, not the ordinal: a sensor list that
        // was rebuilt between building the menu and clicking it would
        // otherwise select a different sensor than the one clicked.
        let selection = if sensor_index == 0 {
            None
        } else {
            state.live_ids.get(&tag).cloned().or_else(|| {
                state
                    .store
                    .get(category)
                    .sensors
                    .get(sensor_index - 1)
                    .map(|sensor| sensor.id.clone())
            })
        };
        state.prefs.toggle_selection(category, selection.as_deref());
        // Picking a row can add or remove the category, so the title and the
        // fixed length both come from the path that watches the category set.
        show_latest(state, true);
        return;
    }
    if tag == 0 {
        return;
    }
    match tag {
        TAG_LOGIN => {
            let enabled = !state.prefs.start_at_login;
            // The checkmark follows the preference, not the agent file: macOS
            // deletes that file on its own and the app rebuilds it from here
            // on the next launch. A failed registration keeps the old value,
            // so the menu never claims a login item it could not write.
            match crate::login::apply(enabled) {
                Ok(()) => state.prefs.set_start_at_login(enabled),
                Err(error) => eprintln!("start at login: {error}"),
            }
            if !state.login_item.is_null() {
                unsafe {
                    objc::send_void_int(
                        state.login_item,
                        "setState:",
                        if state.prefs.start_at_login { 1 } else { 0 },
                    );
                }
            }
            return;
        }
        TAG_FIXED_WIDTH => {
            let enabled = !state.prefs.fixed_status_width;
            state.prefs.set_fixed_status_width(enabled);
            // Both directions have to take effect now: pinning measures the
            // current readings, unpinning hands the width back to AppKit.
            state.last_title.clear();
            let slots = status_slots(state);
            apply_status_length(state, &slots);
            update_status_titles(state);
            return;
        }
        TAG_QUIT => unsafe {
            let app = objc::send(objc::class("NSApplication"), "sharedApplication");
            objc::send_void_obj(app, "terminate:", std::ptr::null_mut());
        },
        TAG_UPDATE => {
            // The worker and its polling timer are the only things this starts;
            // the row's next open redraws from whatever state they leave.
            let controller = state.controller;
            state.updates.activate(controller);
            refresh_update_row(state);
        }
        other if other <= -31 && other >= -40 => {
            let seconds = (-other - 30) as f64;
            state.prefs.set_interval(seconds);
            schedule_ticks(state);
        }
        _ => {}
    }
    state.last_title.clear();
    update_status_titles(state);
}

// MARK: - Building attributed strings

struct Attributed {
    string: Obj,
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
            self.empty = false;
        }
    }

    fn append_symbol(&mut self, category: Category) {
        unsafe {
            let Some(font) = crate::icons::font(STATUS_ICON_SIZE) else { return };
            let icon = category.icon().to_string();
            let attributes = objc::send(objc::class("NSMutableDictionary"), "dictionary");
            objc::send_void_obj_obj(attributes, "setObject:forKey:", font, objc::nsstring("NSFont"));
            let piece = objc::send_obj_obj(
                objc::alloc("NSAttributedString"),
                "initWithString:attributes:",
                objc::nsstring(&icon),
                attributes,
            );
            objc::send_void_obj(self.string, "appendAttributedString:", piece);
            objc::release(piece);
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

    /// A row without a leading icon.
    fn row(left: &str, right: &str, column_right: f64, dim: bool) -> Obj {
        Attributed::row_with_symbol(None, left, right, column_right, dim)
    }

/// Two-column row: label, a tab, then the value ending on `column_right`.
    /// The tab is placed where this row's value has to start, not on the
    /// column edge with right alignment. A right-aligned stop makes AppKit
    /// reserve the tab position *and* the value's width when it sizes the
    /// menu, which leaves a dead band between the values and the menu edge.
    fn row_with_symbol(
        icon: Option<char>,
        left: &str,
        right: &str,
        column_right: f64,
        dim: bool,
    ) -> Obj {
        unsafe {
            let text = match icon {
                Some(icon) => format!("{icon}\t{left}\t{right}"),
                None => format!("{left}\t{right}"),
            };
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

            // Glyphs from the icon font all advance the same width, so the
            // name can sit on a tab stop instead of wherever an image ended.
            let value_stop = (column_right - value_width).max(ICON_SLOT + 1.0);
            let paragraph = objc::send(objc::alloc("NSMutableParagraphStyle"), "init");
            let tabs = objc::send(objc::class("NSMutableArray"), "array");
            if icon.is_some() {
                let tab = objc::send_tab(ICON_SLOT);
                objc::send_void_obj(tabs, "addObject:", tab);
                objc::release(tab);
            }
            let tab = objc::send_tab(value_stop);
            objc::send_void_obj(tabs, "addObject:", tab);
            objc::release(tab);
            objc::send_void_obj(paragraph, "setTabStops:", tabs);
            objc::send_void_float(paragraph, "setDefaultTabInterval:", value_stop);

            let string = objc::send_obj(
                objc::alloc("NSMutableAttributedString"),
                "initWithString:",
                objc::nsstring(&text),
            );
            // NSRange counts UTF-16 units, so a non-BMP character in a label must
        // not shift the value's font by one.
        let full_length = text.encode_utf16().count();
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

            if icon.is_some() {
                if let Some(icon_font) = crate::icons::font(ICON_BOX) {
                    objc::send_attr_range(
                        string,
                        "addAttribute:value:range:",
                        objc::nsstring("NSFont"),
                        icon_font,
                        0,
                        1,
                    );
                }
            }

            // The value is always drawn in the font its width was measured
            // with; measuring in one font and drawing in another is what left
            // a different amount of space at the right of every row.
            let value_start = usize::from(icon.is_some()) + left.encode_utf16().count() + 1;
            objc::send_attr_range(
                string,
                "addAttribute:value:range:",
                objc::nsstring("NSFont"),
                value_font,
                value_start,
                right.chars().count(),
            );
            if dim {
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




/// Builds the dropdown and prints it, without creating any status item, so
/// the whole UI can be checked without anything appearing on screen.
pub fn dump_menu() -> String {
    prepare_application();
    unsafe {
        let controller = make_controller();
        let mut store = SensorStore::new();
        store.refresh();
        let prefs = Prefs::load();
        install(AppState {
            store,
            prefs,
            controller,
            status_item: std::ptr::null_mut(),
            live_items: HashMap::new(),
            open_menu: std::ptr::null_mut(),
            row_tab: 0.0,
            sensor_tabs: [0.0; Category::COUNT],
            submenus: HashMap::new(),
            open_submenu: None,
            poller: None,
            tick_timer: std::ptr::null_mut(),
            collect_timer: std::ptr::null_mut(),
            collect_waits: 0,
            last_title: String::new(),
            login_item: std::ptr::null_mut(),
            last_signature: String::new(),
            live_ids: HashMap::new(),
            row_values: HashMap::new(),
            poller_restarts: 0,
            status_slots: HashMap::new(),
            status_separator: 0.0,
            status_length: 0.0,
            update_item: std::ptr::null_mut(),
            reopen_timer: std::ptr::null_mut(),
            updates: Updates::idle(),
        });

        let menu = make_menu(controller);
        let mut out = String::from("# Vitals menu structure (live NSMenu objects)\n");
        with_state(|state| {
            populate(state, menu);
            out.push_str(&format!(
                "# menu bar slots: {}\n# refresh interval: {} s\n# selections: {}\n# status item width: {:.1} pt (fixed)\n# updates: {}\n",
                state
                    .prefs
                    .ordered_menu_bar()
                    .iter()
                    .map(|c| c.title())
                    .collect::<Vec<_>>()
                    .join(", "),
                state.prefs.interval,
                diag::selection_summary(&state.prefs),
                measured_status_length(&status_slots(state)),
                state.updates.summary()
            ));
            fill_all_submenus(state, menu);
            let icon_sizes = ALL_CATEGORIES
                .iter()
                .map(|category| {
                    let font = crate::icons::font(ICON_BOX);
                    format!("{}/{}", category.title(), if font.is_some() { "ok" } else { "missing" })
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!("\n## Icons\n   {icon_sizes}\n"));
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
                let symbol = Some(category.icon());
                let row = Attributed::row_with_symbol(
                    symbol,
                    category.title(),
                    text,
                    state.row_tab,
                    false,
                );
                let row_width = objc::send_size(row, "size").width;
                let name_index = usize::from(symbol.is_some());
                let name_x = name_position(row, name_index);
                objc::release(row);
                out.push_str(&format!(
                    "   {:<12} name at {:>5.1}, label ends {:>6.1}, value starts {:>6.1}, gap {:>5.1}, ends {:>6.1}, row {:>6.1}\n",
                    category.title(),
                    name_x,
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
        objc::release(tab);
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
        install(AppState {
            store,
            prefs,
            controller,
            status_item: std::ptr::null_mut(),
            live_items: HashMap::new(),
            open_menu: std::ptr::null_mut(),
            row_tab: 0.0,
            sensor_tabs: [0.0; Category::COUNT],
            submenus: HashMap::new(),
            open_submenu: None,
            poller: None,
            tick_timer: std::ptr::null_mut(),
            collect_timer: std::ptr::null_mut(),
            collect_waits: 0,
            last_title: String::new(),
            login_item: std::ptr::null_mut(),
            last_signature: String::new(),
            live_ids: HashMap::new(),
            row_values: HashMap::new(),
            poller_restarts: 0,
            status_slots: HashMap::new(),
            status_separator: 0.0,
            status_length: 0.0,
            update_item: std::ptr::null_mut(),
            reopen_timer: std::ptr::null_mut(),
            updates: Updates::idle(),
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
                // Draw through the text system, which lays out attachments the
                // way a menu item does; plain -drawAtPoint: does not.
                let storage = objc::send_obj(
                    objc::alloc("NSTextStorage"),
                    "initWithAttributedString:",
                    *title,
                );
                let manager = objc::send(objc::alloc("NSLayoutManager"), "init");
                objc::send_void_obj(storage, "addLayoutManager:", manager);
                let container = objc::send_obj_size(
                    objc::alloc("NSTextContainer"),
                    "initWithSize:",
                    width,
                    row_height + 6.0,
                );
                objc::send_void_obj(manager, "addTextContainer:", container);
                let glyphs = objc::send_int_result(manager, "numberOfGlyphs") as usize;
                objc::send_void_range_point(manager, "drawGlyphsForGlyphRange:atPoint:", 0, glyphs, 4.0, y);
                objc::release(manager);
                objc::release(storage);
                objc::release(container);
            }
            objc::send_void(image, "unlockFocus");
            let tiff = objc::send(image, "TIFFRepresentation");
            let bitmap = objc::send_obj(objc::class("NSBitmapImageRep"), "imageRepWithData:", tiff);
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

/// Fills every submenu, for the diagnostics dump. The app itself fills one at
/// a time, when a submenu is opened; the dump has no status item to ask, so it
/// passes the menu it built.
fn fill_all_submenus(state: &mut AppState, menu: Obj) {
    if menu.is_null() {
        return;
    }
    unsafe {
        let items = objc::send(menu, "itemArray");
        let count = objc::send_int_result(items, "count");
        for index in 0..count {
            let row = objc::send_int(items, "objectAtIndex:", index);
            if row.is_null() {
                continue;
            }
            let submenu = objc::send(row, "submenu");
            let Some(category) = state.submenus.get(&(submenu as usize)).copied() else {
                continue;
            };
            let reading = state.store.get(category).clone();
            populate_submenu(state, submenu, category, &reading);
        }
    }
}

/// Times building the dropdown and one submenu, in milliseconds.
pub fn bench_menu() -> String {
    prepare_application();
    unsafe {
        let controller = make_controller();
        let mut store = SensorStore::new();
        store.refresh();
        let prefs = Prefs::load();
        install(AppState {
            store,
            prefs,
            controller,
            status_item: std::ptr::null_mut(),
            live_items: HashMap::new(),
            open_menu: std::ptr::null_mut(),
            row_tab: 0.0,
            sensor_tabs: [0.0; Category::COUNT],
            submenus: HashMap::new(),
            open_submenu: None,
            poller: None,
            tick_timer: std::ptr::null_mut(),
            collect_timer: std::ptr::null_mut(),
            collect_waits: 0,
            last_title: String::new(),
            login_item: std::ptr::null_mut(),
            last_signature: String::new(),
            live_ids: HashMap::new(),
            row_values: HashMap::new(),
            poller_restarts: 0,
            status_slots: HashMap::new(),
            status_separator: 0.0,
            status_length: 0.0,
            update_item: std::ptr::null_mut(),
            reopen_timer: std::ptr::null_mut(),
            updates: Updates::idle(),
        });
        with_state(|state| {
            let menu = make_menu(state.controller);
            let runs = 10;
            let start = Instant::now();
            for _ in 0..runs {
                populate(state, menu);
            }
            let dropdown_ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;

            let submenu = objc::send_obj(
                objc::alloc("NSMenu"),
                "initWithTitle:",
                objc::nsstring("Temperature"),
            );
            let reading = state.store.get(Category::Temperature).clone();
            let start = Instant::now();
            for _ in 0..runs {
                populate_submenu(state, submenu, Category::Temperature, &reading);
            }
            let submenu_ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;

            let start = Instant::now();
            for _ in 0..runs {
                refresh_values(state);
            }
            let refresh_ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;

            let start = Instant::now();
            for _ in 0..5 {
                state.store.refresh();
            }
            let sensor_ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;

            let shown = state
                .prefs
                .ordered_menu_bar()
                .into_iter()
                .map(|category| (category, state.prefs.selection(category).cloned()))
                .collect::<Vec<_>>();
            let start = Instant::now();
            for _ in 0..5 {
                state.store.refresh_shown(&shown);
            }
            let shown_ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;

            let one_sensor = vec![(Category::Temperature, Some("TAOL".to_string()))];
            let start = Instant::now();
            for _ in 0..5 {
                state.store.refresh_shown(&one_sensor);
            }
            let single_ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;

            let rows = objc::send_int_result(
                objc::send(menu, "itemArray"),
                "count",
            );
            let sample = shown
                .iter()
                .map(|(category, selection)| {
                    let reading = state.store.get(*category);
                    format!(
                        "{}={}",
                        category.title(),
                        reading.reading_for(selection.as_deref()).text
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "dropdown build: {dropdown_ms:.2} ms ({rows} rows)\n\
                 temperature submenu build: {submenu_ms:.2} ms ({} rows)\n\
                 live refresh: {refresh_ms:.2} ms\n\
                 sensor poll (all nine): {sensor_ms:.2} ms\n\
                 sensor poll (menu bar only): {shown_ms:.2} ms\n\
                 sensor poll (one sensor): {single_ms:.2} ms\n\
                 menu bar after a subset poll: {sample}\n",
                reading.sensors.len() + 2
            )
        })
    }
}

/// Where the category name actually starts in a rendered row, measured through
/// the text system rather than guessed from the icon's width.
unsafe fn name_position(row: Obj, index: usize) -> f64 {
    let storage = objc::send_obj(objc::alloc("NSTextStorage"), "initWithAttributedString:", row);
    let manager = objc::send(objc::alloc("NSLayoutManager"), "init");
    objc::send_void_obj(storage, "addLayoutManager:", manager);
    let container = objc::send_obj_size(
        objc::alloc("NSTextContainer"),
        "initWithSize:",
        400.0,
        30.0,
    );
    objc::send_void_obj(manager, "addTextContainer:", container);
    let glyph = objc::send_int_obj_result2(manager, "glyphIndexForCharacterAtIndex:", index);
    let x = objc::send_point_int(manager, "locationForGlyphAtIndex:", glyph).width;
    objc::release(manager);
    objc::release(storage);
    objc::release(container);
    x
}
