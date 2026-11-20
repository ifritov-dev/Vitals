import AppKit
import Foundation
import VitalsCore

/// Minimal CLI plumbing so the same binary can print a sensor dump.
enum Output {
    static func value(for flag: String) -> String? {
        let arguments = CommandLine.arguments
        guard let index = arguments.firstIndex(of: flag), index + 1 < arguments.count else { return nil }
        return arguments[index + 1]
    }

    static func emit(_ text: String) {
        if let path = value(for: "--out") {
            let expanded = (path as NSString).expandingTildeInPath
            try? text.write(toFile: expanded, atomically: true, encoding: .utf8)
            print("wrote \(expanded)")
        } else {
            print(text)
        }
    }
}

if CommandLine.arguments.contains("--dump") {
    let store = SensorStore()
    let samples = Int(Output.value(for: "--samples") ?? "1") ?? 1
    let interval = Double(Output.value(for: "--interval") ?? "2") ?? 2
    Output.emit(Diagnostics.sensorDump(store: store, samples: samples, interval: interval))
    exit(0)
}

if CommandLine.arguments.contains("--selftest") {
    let result = SelfTest.run()
    for failure in result.failures {
        print("FAIL  \(failure)")
    }
    print("\(result.passed) checks passed, \(result.failures.count) failed")
    exit(result.failures.isEmpty ? 0 : 1)
}

if let categoryArg = Output.value(for: "--set-selection"),
   let category = CategoryID(rawValue: categoryArg) {
    // Same write path the menu uses: persists through Preferences.
    let prefs = Preferences()
    let sensor = Output.value(for: "--sensor")
    prefs.setSelection(sensor == "average" ? nil : sensor, for: category)
    if !prefs.menuBarCategories.contains(category) {
        prefs.setMenuBarCategories(prefs.menuBarCategories + [category])
    }
    print("saved \(category.rawValue) -> \(sensor ?? "average")")
    exit(0)
}

if CommandLine.arguments.contains("--clear-config") {
    if let domain = Bundle.main.bundleIdentifier {
        UserDefaults.standard.removePersistentDomain(forName: domain)
    }
    print("cleared")
    exit(0)
}

if CommandLine.arguments.contains("--print-config") {
    let prefs = Preferences()
    print("menuBarCategories = \(prefs.menuBarCategories.map(\.rawValue))")
    print("refreshInterval   = \(prefs.refreshInterval) s")
    print("showIcons         = \(prefs.showIcons)")
    let selections = CategoryID.allCases.map { "\($0.rawValue)=\(prefs.selection(for: $0) ?? "average")" }
    print("selections        = \(selections.joined(separator: ", "))")
    print("defaults domain   = com.daniel.vitals")
    exit(0)
}

let application = NSApplication.shared
application.setActivationPolicy(.accessory)
let delegate = AppDelegate()
application.delegate = delegate
application.run()
