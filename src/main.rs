mod diag;
mod icons;
mod login;
mod model;
mod objc;
mod poller;
mod prefs;
mod selftest;
mod sensors;
mod smc;
mod store;
mod ui;
mod update;

use model::Category;
use prefs::Prefs;
use store::SensorStore;

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn emit(text: &str, path: Option<String>) {
    match path {
        Some(path) => match std::fs::write(&path, text) {
            Ok(()) => println!("wrote {path}"),
            Err(error) => {
                eprintln!("could not write {path}: {error}");
                std::process::exit(1);
            }
        },
        None => print!("{text}"),
    }
}

/// Outside the app bundle `+standardUserDefaults` is a different domain, so a
/// diagnostic that silently read it would report defaults as the user's
/// settings. Say so instead.
fn warn_outside_bundle(command: &str) {
    if !login::bundled() {
        eprintln!("{command}: run the installed app to read its preferences");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--selftest") {
        let outcome = selftest::run();
        for failure in &outcome.failures {
            println!("FAIL  {failure}");
        }
        println!(
            "{} checks passed, {} failed",
            outcome.passed,
            outcome.failures.len()
        );
        std::process::exit(if outcome.failures.is_empty() { 0 } else { 1 });
    }

    if args.iter().any(|a| a == "--dump") {
        let samples = arg_value(&args, "--samples")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1usize);
        let interval = arg_value(&args, "--interval")
            .and_then(|v| v.parse().ok())
            .unwrap_or(2.0f64);
        // NSUserDefaults wants an application instance to exist.
        ui::prepare_application();
        let mut store = SensorStore::new();
        emit(
            &diag::sensor_dump(&mut store, samples, interval),
            arg_value(&args, "--out"),
        );
        return;
    }

    if args.iter().any(|a| a == "--dump-menu") {
        warn_outside_bundle("--dump-menu");
        ui::prepare_application();
        let text = ui::dump_menu();
        emit(&text, arg_value(&args, "--out"));
        return;
    }

    let headless = args.iter().any(|a| {
        a == "--print-config"
            || a == "--check-updates"
            || a == "--bench-menu"
            || a == "--login-status"
            || a.starts_with("--set-")
            || a.starts_with("--render")
    });
    if !headless && !login::claim_single_instance() {
        return;
    }

    // The check is one bounded network call and needs none of the app's state,
    // so a diagnostic can report exactly what the menu would show.
    if args.iter().any(|a| a == "--check-updates") {
        println!("current version {}", update::current_version());
        match update::check() {
            Ok(Some(found)) => println!("update available: {} {}", found.version, found.dmg_url),
            Ok(None) => println!("up to date"),
            Err(error) => {
                eprintln!("update check failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.iter().any(|a| a == "--login-status") {
        ui::prepare_application();
        let pref = if login::bundled() {
            Prefs::load().start_at_login.to_string()
        } else {
            "unknown outside the app bundle".to_string()
        };
        print!(
            "start at login: {pref}\nagent plist:    {}\nlaunchd job:    {}\n",
            if login::installed() { "present" } else { "missing" },
            if login::loaded() { "loaded" } else { "not loaded" },
        );
        return;
    }

    if args.iter().any(|a| a == "--set-login") {
        let Some(value) = arg_value(&args, "--set-login") else {
            eprintln!("--set-login takes on or off");
            std::process::exit(2);
        };
        let enabled = match value.as_str() {
            "on" | "true" | "1" => true,
            "off" | "false" | "0" => false,
            other => {
                eprintln!("--set-login takes on or off, not {other}");
                std::process::exit(2);
            }
        };
        ui::prepare_application();
        // The flag lives in the app's own preference domain: wiring the agent
        // from a process outside it would be undone by the next launch.
        if !login::bundled() {
            eprintln!("--set-login must run from the installed app bundle");
            std::process::exit(1);
        }
        match login::apply(enabled) {
            Ok(()) => {
                Prefs::load().set_start_at_login(enabled);
                println!("start at login: {enabled}");
            }
            Err(error) => {
                eprintln!("failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.iter().any(|a| a == "--bench-menu") {
        warn_outside_bundle("--bench-menu");
        ui::prepare_application();
        print!("{}", ui::bench_menu());
        return;
    }

    if let Some(path) = arg_value(&args, "--render-rows") {
        warn_outside_bundle("--render-rows");
        ui::prepare_application();
        ui::render_rows(&path);
        return;
    }

    if args.iter().any(|a| a == "--print-config") {
        warn_outside_bundle("--print-config");
        ui::prepare_application();
        let prefs = Prefs::load();
        print!("{}", diag::config_dump(&prefs));
        return;
    }

    if args.iter().any(|a| a == "--set-selection") {
        let Some(category_key) = arg_value(&args, "--set-selection") else {
            eprintln!("--set-selection takes a category");
            std::process::exit(2);
        };
        let Some(category) = Category::from_key(&category_key) else {
            eprintln!("unknown category {category_key}");
            std::process::exit(2);
        };
        // A write into the wrong domain would report success and change
        // nothing the menu reads.
        if !login::bundled() {
            eprintln!("--set-selection must run from the installed app bundle");
            std::process::exit(1);
        }
        ui::prepare_application();
        let mut prefs = Prefs::load();
        let sensor = arg_value(&args, "--sensor");
        let sensor = sensor.as_deref().filter(|s| *s != "average");
        prefs.set_selection(category, sensor);
        println!(
            "saved {} -> {}",
            category.key(),
            sensor.unwrap_or("average")
        );
        return;
    }

    // macOS deletes the login agent file on its own, so the agent is rebuilt
    // from the preference on every launch instead of read back from disk.
    ui::prepare_application();
    if let Err(error) = login::apply(Prefs::load().start_at_login) {
        eprintln!("start at login: {error}");
    }
    ui::run();
}
