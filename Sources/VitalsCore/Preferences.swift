import Foundation

/// Everything the app remembers between launches.
public final class Preferences {
    public enum Keys {
        public static let menuBarCategories = "menuBarCategories"
        public static let selections = "categorySelections"
        public static let refreshInterval = "refreshInterval"
        public static let showIcons = "showIconsInMenuBar"
        public static let compactSlot = "compactSingleSlot"
    }

    public static let intervalRange: ClosedRange<Double> = 1...10
    public static let defaultMenuBarCategories: [CategoryID] = [.memory, .system, .processor]

    private let defaults: UserDefaults

    public private(set) var menuBarCategories: [CategoryID]
    public private(set) var selections: [CategoryID: String]
    public private(set) var refreshInterval: Double
    public private(set) var showIcons: Bool
    public private(set) var compactSlot: Bool

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults

        if let stored = defaults.array(forKey: Keys.menuBarCategories) as? [String] {
            menuBarCategories = stored.compactMap(CategoryID.init(rawValue:))
        } else {
            menuBarCategories = Preferences.defaultMenuBarCategories
        }

        if let stored = defaults.dictionary(forKey: Keys.selections) as? [String: String] {
            var parsed: [CategoryID: String] = [:]
            for (key, value) in stored {
                if let category = CategoryID(rawValue: key) { parsed[category] = value }
            }
            selections = parsed
        } else {
            selections = [:]
        }

        let storedInterval = defaults.double(forKey: Keys.refreshInterval)
        refreshInterval = storedInterval >= Preferences.intervalRange.lowerBound
            && storedInterval <= Preferences.intervalRange.upperBound
            ? storedInterval : 2

        showIcons = defaults.object(forKey: Keys.showIcons) as? Bool ?? true
        compactSlot = defaults.object(forKey: Keys.compactSlot) as? Bool ?? true
    }

    public func setMenuBarCategories(_ categories: [CategoryID]) {
        menuBarCategories = categories
        save()
    }

    public func selection(for category: CategoryID) -> String? {
        selections[category]
    }

    public func setSelection(_ sensorID: String?, for category: CategoryID) {
        if let sensorID {
            selections[category] = sensorID
        } else {
            selections.removeValue(forKey: category)
        }
        save()
    }

    public func setRefreshInterval(_ seconds: Double) {
        refreshInterval = min(max(seconds, Preferences.intervalRange.lowerBound),
                              Preferences.intervalRange.upperBound)
        save()
    }

    public func setShowIcons(_ value: Bool) {
        showIcons = value
        save()
    }

    public func setCompactSlot(_ value: Bool) {
        compactSlot = value
        save()
    }

    private func save() {
        defaults.set(menuBarCategories.map(\.rawValue), forKey: Keys.menuBarCategories)
        defaults.set(Dictionary(uniqueKeysWithValues: selections.map { ($0.key.rawValue, $0.value) }),
                     forKey: Keys.selections)
        defaults.set(refreshInterval, forKey: Keys.refreshInterval)
        defaults.set(showIcons, forKey: Keys.showIcons)
        defaults.set(compactSlot, forKey: Keys.compactSlot)
    }
}
