import Foundation

/// Assertions for the pure logic, runnable from the app itself.
///
/// This exists instead of an XCTest target because the machine has Command
/// Line Tools only: XCTest ships with Xcode.
public enum SelfTest {
    public static func run() -> (passed: Int, failures: [String]) {
        var passed = 0
        var failures: [String] = []

        func check(_ name: String, _ condition: @autoclosure () -> Bool) {
            if condition() {
                passed += 1
            } else {
                failures.append(name)
            }
        }

        func close(_ name: String, _ actual: Double?, _ expected: Double, _ tolerance: Double = 0.0001) {
            guard let actual else {
                failures.append("\(name): got nil, expected \(expected)")
                return
            }
            if abs(actual - expected) <= tolerance {
                passed += 1
            } else {
                failures.append("\(name): got \(actual), expected \(expected)")
            }
        }

        // sp78 is signed 7.8 fixed point; the sign bit is what usually breaks.
        close("sp78 positive", SMCDecoder.decode(type: "sp78", bytes: [0x34, 0x12], size: 2), 52.0703125)
        close("sp78 negative", SMCDecoder.decode(type: "sp78", bytes: [0xC0, 0x00], size: 2), -64)
        close("sp78 minus one", SMCDecoder.decode(type: "sp78", bytes: [0xFF, 0x00], size: 2), -1)
        // fpe2 is unsigned 14.2 fixed point.
        close("fpe2 3072", SMCDecoder.decode(type: "fpe2", bytes: [0x0C, 0x00], size: 2), 768)
        close("fpe2 1.0", SMCDecoder.decode(type: "fpe2", bytes: [0x00, 0x04], size: 2), 1)
        // Types arrive padded to four characters.
        close("padded flt", SMCDecoder.decode(type: "flt ", bytes: [0xBE, 0x8D, 0x5F, 0x42], size: 4), 55.888, 0.01)
        close("padded ui16", SMCDecoder.decode(type: "ui16 ", bytes: [0x0E, 0x2F], size: 2), 3631)
        close("ui8", SMCDecoder.decode(type: "ui8", bytes: [0xFF], size: 1), 255)
        close("si8", SMCDecoder.decode(type: "si8", bytes: [0xFF], size: 1), -1)
        close("si16", SMCDecoder.decode(type: "si16", bytes: [0xFF, 0xFF], size: 2), -1)
        close("ui32 key count", SMCDecoder.decode(type: "ui32", bytes: [0x00, 0x00, 0x08, 0x85], size: 4), 2181)

        var bits = 71.5.bitPattern
        var doubleBytes: [UInt8] = []
        for _ in 0..<8 {
            doubleBytes.append(UInt8(bits & 0xFF))
            bits >>= 8
        }
        close("ioft double", SMCDecoder.decode(type: "ioft", bytes: doubleBytes, size: 8), 71.5)
        check("unknown type is nil", SMCDecoder.decode(type: "ch8*", bytes: [1, 2, 3, 4], size: 4) == nil)
        check("short flt is nil", SMCDecoder.decode(type: "flt", bytes: [0, 0], size: 2) == nil)

        check("format 0 B", Format.scaledBytes(0) == "0 B")
        check("format 812 B", Format.scaledBytes(812) == "812 B")
        check("format 1.5 KB", Format.scaledBytes(1536) == "1.50 KB")
        check("format 12.4 MB", Format.scaledBytes(12.4 * 1024 * 1024) == "12.4 MB")
        check("format compact percent", Format.percent(57.859).compact == "58%")
        check("format percent text", Format.percent(57.859).text == "57.9%")
        check("format temperature", Format.temperature(51.82).text == "51.8 °C")
        check("no data marker", !ReadingValue.noData.hasData)
        check("empty category says no data", CategoryReading.empty(.fan).reading(for: nil).text == "No data")

        let suiteName = "vitals.selftest.\(UUID().uuidString)"
        if let defaults = UserDefaults(suiteName: suiteName) {
            defer { defaults.removePersistentDomain(forName: suiteName) }
            let prefs = Preferences(defaults: defaults)
            check("default menu bar slots", prefs.menuBarCategories == Preferences.defaultMenuBarCategories)
            check("default interval", prefs.refreshInterval == 2)

            prefs.setMenuBarCategories([.temperature, .network])
            prefs.setSelection("TC10", for: .temperature)
            prefs.setRefreshInterval(5)
            prefs.setShowIcons(false)

            let reloaded = Preferences(defaults: defaults)
            check("categories persist", reloaded.menuBarCategories == [.temperature, .network])
            check("selection persists", reloaded.selection(for: .temperature) == "TC10")
            check("interval persists", reloaded.refreshInterval == 5)
            check("icons persist", reloaded.showIcons == false)

            reloaded.setSelection(nil, for: .temperature)
            check("selection clears", Preferences(defaults: defaults).selection(for: .temperature) == nil)

            reloaded.setRefreshInterval(30)
            check("interval clamps high", reloaded.refreshInterval == 10)
            reloaded.setRefreshInterval(0)
            check("interval clamps low", reloaded.refreshInterval == 1)
        } else {
            failures.append("could not create a scratch UserDefaults suite")
        }

        return (passed, failures)
    }
}
