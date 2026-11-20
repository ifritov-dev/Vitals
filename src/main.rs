mod diag;
mod model;
mod objc;
mod prefs;
mod selftest;
mod sensors;
mod smc;
mod store;
mod ui;

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
            Err(error) => eprintln!("could not write {path}: {error}"),
        },
        None => print!("{text}"),
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
        ui::prepare_application();
        let text = ui::dump_menu();
        emit(&text, arg_value(&args, "--out"));
        return;
    }

    if args.iter().any(|a| a == "--print-config") {
        ui::prepare_application();
        let prefs = Prefs::load();
        print!("{}", diag::config_dump(&prefs));
        return;
    }

    if let Some(category_key) = arg_value(&args, "--set-selection") {
        ui::prepare_application();
        if let Some(category) = Category::from_key(&category_key) {
            let mut prefs = Prefs::load();
            let sensor = arg_value(&args, "--sensor");
            let sensor = sensor.as_deref().filter(|s| *s != "average");
            prefs.set_selection(category, sensor);
            println!(
                "saved {} -> {}",
                category.key(),
                sensor.unwrap_or("average")
            );
        }
        return;
    }

    ui::run();
}
