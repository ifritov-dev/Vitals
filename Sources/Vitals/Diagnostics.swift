import AppKit
import VitalsCore

/// Text renders of what the app is showing, so behaviour can be checked
/// without photographing the screen.
enum Diagnostics {
    static func hostSummary() -> String {
        func sysctlString(_ name: String) -> String {
            var size = 0
            guard sysctlbyname(name, nil, &size, nil, 0) == 0, size > 0 else { return "unknown" }
            var buffer = [CChar](repeating: 0, count: size)
            guard sysctlbyname(name, &buffer, &size, nil, 0) == 0 else { return "unknown" }
            return String(cString: buffer)
        }
        let version = ProcessInfo.processInfo.operatingSystemVersionString
        return "\(sysctlString("machdep.cpu.brand_string")) · \(version) · \(Host.current().localizedName ?? "this Mac")"
    }

    static func sensorDump(store: SensorStore, samples: Int, interval: Double) -> String {
        var out = "# Vitals sensor dump\n"
        out += "# host: \(hostSummary())\n"
        out += "# SMC: " + (store.smcAvailable ? "AppleSMC opened unprivileged" : "unavailable")
        out += ", \(store.smcKeyCount) keys total, \(store.smcSensorKeyCount) polled per refresh\n"

        for sample in 1...max(1, samples) {
            if sample > 1 {
                Thread.sleep(forTimeInterval: interval)
            }
            store.refresh()
            let formatter = ISO8601DateFormatter()
            out += "\n# sample \(sample) at \(formatter.string(from: Date()))\n"
            let readings = store.current()
            for category in CategoryID.allCases {
                let reading = readings[category] ?? .empty(category)
                let header: String
                if reading.average.hasData {
                    header = "## \(category.title)  —  average \(reading.average.text)  ·  \(reading.sensors.count) sensor(s)"
                } else {
                    header = "## \(category.title)  —  No data"
                }
                out += header + "\n"
                for sensor in reading.sensors {
                    let left = sensor.key.isEmpty ? sensor.label : "\(sensor.key)  \(sensor.label)"
                    out += "   " + left.padding(toLength: max(30, left.count + 1), withPad: " ", startingAt: 0)
                    out += sensor.reading.text + "\n"
                }
            }
        }
        return out
    }

    /// Walks the real NSMenu the popup uses: titles, checkmarks, submenus.
    static func menuDump(_ menu: NSMenu, prefs: Preferences, frames: [(String, NSRect?, Bool)]) -> String {
        var out = "# Vitals menu structure (live NSMenu objects)\n"
        out += "# menu bar slots: " + prefs.menuBarCategories.map(\.title).joined(separator: ", ") + "\n"
        out += "# refresh interval: \(Int(prefs.refreshInterval)) s\n"
        out += "# selections: "
        let selected = CategoryID.allCases.compactMap { category -> String? in
            guard let sensor = prefs.selection(for: category) else { return nil }
            return "\(category.title)=\(sensor)"
        }
        out += (selected.isEmpty ? "all Average" : selected.joined(separator: ", ")) + "\n"
        out += "\n## NSStatusItem frames\n"
        for (name, frame, visible) in frames {
            let description = frame.map { "(\(Int($0.origin.x)), \(Int($0.origin.y))) \(Int($0.width))×\(Int($0.height))" } ?? "no window"
            out += "   \(name.padding(toLength: 20, withPad: " ", startingAt: 0)) \(description)  visible=\(visible)\n"
        }
        out += "\n## Menu widths\n"
        out += String(format: "   dropdown: %.0f × %.0f pt\n", menu.size.width, menu.size.height)
        for item in menu.items where item.submenu != nil {
            let size = item.submenu!.size
            out += String(format: "   %@ submenu: %.0f × %.0f pt\n",
                          item.title.padding(toLength: 14, withPad: " ", startingAt: 0),
                          size.width, size.height)
        }
        out += "\n## Dropdown\n"
        walk(menu, depth: 1, into: &out)
        return out
    }

    private static func walk(_ menu: NSMenu, depth: Int, into out: inout String) {
        for item in menu.items {
            let indent = String(repeating: "   ", count: depth)
            if item.isSeparatorItem {
                out += indent + "────────\n"
                continue
            }
            let mark = item.state == .on ? "[✓]" : "[ ]"
            let enabled = item.isEnabled ? "" : " (disabled)"
            let text = (item.attributedTitle?.string ?? item.title)
                .replacingOccurrences(of: "\t", with: "   ")
                .trimmingCharacters(in: .whitespaces)
            let arrow = item.submenu != nil ? "  ›" : ""
            out += "\(indent)\(mark) \(text)\(arrow)\(enabled)\n"
            if let submenu = item.submenu {
                walk(submenu, depth: depth + 1, into: &out)
            }
        }
    }
}
