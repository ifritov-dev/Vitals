import AppKit
import VitalsCore

private final class SensorSelection: NSObject {
    let category: CategoryID
    let sensorID: String?

    init(category: CategoryID, sensorID: String?) {
        self.category = category
        self.sensorID = sensorID
    }
}

/// Builds the dropdown for a status item and keeps it live while it is open.
///
/// Each status item owns its own NSMenu instance and this object is its
/// delegate, so AppKit anchors the popup under the slot that was clicked.
/// The contents are filled in `menuWillOpen`; while the menu tracks,
/// `refreshValues()` only rewrites titles, so open submenus survive a refresh.
final class VitalsMenu: NSObject, NSMenuDelegate {
    private let store: SensorStore
    private let prefs: Preferences

    private var rowItems: [CategoryID: NSMenuItem] = [:]
    private var averageItems: [CategoryID: NSMenuItem] = [:]
    private var sensorItems: [CategoryID: [String: NSMenuItem]] = [:]
    private var intervalItems: [NSMenuItem] = []

    private(set) var openMenu: NSMenu?

    /// Gap between the label column and the value column.
    private let columnGap: CGFloat = 22

    var onSelectionChanged: (() -> Void)?
    var onOpenSettings: (() -> Void)?
    var onQuit: (() -> Void)?
    var onIntervalChanged: (() -> Void)?

    init(store: SensorStore, prefs: Preferences) {
        self.store = store
        self.prefs = prefs
        super.init()
    }

    /// A menu ready to hand to a status item.
    func makeAttachedMenu() -> NSMenu {
        let menu = NSMenu(title: "Vitals")
        menu.delegate = self
        menu.autoenablesItems = false
        return menu
    }

    func menuWillOpen(_ menu: NSMenu) {
        populate(menu)
        openMenu = menu
    }

    func menuDidClose(_ menu: NSMenu) {
        openMenu = nil
    }

    /// Fills a menu without waiting for it to open; used by the headless dump.
    func populateForDump(_ menu: NSMenu) {
        populate(menu)
    }

    private func populate(_ menu: NSMenu) {
        menu.removeAllItems()
        rowItems = [:]
        averageItems = [:]
        sensorItems = [:]
        intervalItems = []

        let readings = store.current()
        let rowTabStop = tabStop(for: CategoryID.allCases.map(\.title))
        for category in CategoryID.allCases {
            let reading = readings[category] ?? .empty(category)
            let item = NSMenuItem(title: category.title, action: nil, keyEquivalent: "")
            item.image = Icons.symbol(for: category)
            let submenu = NSMenu(title: category.title)
            submenu.autoenablesItems = false
            fillSensorMenu(submenu, for: category, reading: reading)
            item.submenu = submenu
            item.attributedTitle = rowTitle(for: category, reading: reading, tabStop: rowTabStop)
            menu.addItem(item)
            rowItems[category] = item
        }

        menu.addItem(.separator())

        let settings = NSMenuItem(title: "Settings…", action: #selector(openSettings), keyEquivalent: ",")
        settings.target = self
        settings.image = Icons.symbolNamed("gearshape")
        menu.addItem(settings)

        let intervalItem = NSMenuItem(title: "Refresh Interval", action: nil, keyEquivalent: "")
        intervalItem.image = Icons.symbolNamed("timer")
        let intervalMenu = NSMenu(title: "Refresh Interval")
        for seconds in 1...10 {
            let entry = NSMenuItem(title: "\(seconds) s", action: #selector(intervalChosen(_:)), keyEquivalent: "")
            entry.target = self
            entry.tag = seconds
            entry.state = Int(prefs.refreshInterval) == seconds ? .on : .off
            intervalMenu.addItem(entry)
            intervalItems.append(entry)
        }
        intervalItem.submenu = intervalMenu
        menu.addItem(intervalItem)

        menu.addItem(.separator())

        let quit = NSMenuItem(title: "Quit Vitals", action: #selector(quit), keyEquivalent: "q")
        quit.target = self
        menu.addItem(quit)
    }

    private func fillSensorMenu(_ menu: NSMenu, for category: CategoryID, reading: CategoryReading) {
        let active = prefs.selection(for: category)
        let tabStop = tabStop(for: ["Average"] + reading.sensors.map { sensorLabel(for: $0) })
        let average = NSMenuItem(title: "Average", action: #selector(selectSensor(_:)), keyEquivalent: "")
        average.target = self
        average.representedObject = SensorSelection(category: category, sensorID: nil)
        average.state = active == nil ? .on : .off
        average.attributedTitle = sensorTitle(label: "Average", key: nil,
                                              value: reading.average.hasData ? reading.average.text : "No data",
                                              tabStop: tabStop)
        menu.addItem(average)
        averageItems[category] = average

        if reading.sensors.isEmpty {
            menu.addItem(.separator())
            let none = NSMenuItem(title: "No data", action: nil, keyEquivalent: "")
            none.isEnabled = false
            menu.addItem(none)
            sensorItems[category] = [:]
            return
        }

        menu.addItem(.separator())
        var index: [String: NSMenuItem] = [:]
        for sensor in reading.sensors {
            let item = NSMenuItem(title: sensor.label, action: #selector(selectSensor(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = SensorSelection(category: category, sensorID: sensor.id)
            item.state = active == sensor.id ? .on : .off
            item.attributedTitle = sensorTitle(label: sensor.label, key: displayKey(for: sensor),
                                               value: sensor.reading.text, tabStop: tabStop)
            menu.addItem(item)
            index[sensor.id] = item
        }
        sensorItems[category] = index
    }

    private func displayKey(for sensor: SensorReading) -> String? {
        let key = sensor.key
        guard !key.isEmpty,
              key.caseInsensitiveCompare(sensor.label) != .orderedSame,
              key.count >= 2
        else { return nil }
        return key
    }

    // MARK: Live updates

    /// Rewrites the value column of every row and sensor entry in place.
    func refreshValues() {
        guard openMenu != nil else { return }
        let readings = store.current()
        let rowTabStop = tabStop(for: CategoryID.allCases.map(\.title))
        for category in CategoryID.allCases {
            let reading = readings[category] ?? .empty(category)
            if let row = rowItems[category] {
                let title = rowTitle(for: category, reading: reading, tabStop: rowTabStop)
                if row.attributedTitle?.string != title.string { row.attributedTitle = title }
            }
            if let average = averageItems[category] {
                let text = reading.average.hasData ? reading.average.text : "No data"
                let tabStop = tabStop(for: ["Average"] + reading.sensors.map { sensorLabel(for: $0) })
                let title = sensorTitle(label: "Average", key: nil, value: text, tabStop: tabStop)
                if average.attributedTitle?.string != title.string { average.attributedTitle = title }
            }
            for sensor in reading.sensors {
                guard let item = sensorItems[category]?[sensor.id] else { continue }
                let tabStop = tabStop(for: ["Average"] + reading.sensors.map { sensorLabel(for: $0) })
                let title = sensorTitle(label: sensor.label, key: displayKey(for: sensor),
                                        value: sensor.reading.text, tabStop: tabStop)
                if item.attributedTitle?.string != title.string { item.attributedTitle = title }
            }
        }
    }

    // MARK: Titles

    private func rowTitle(for category: CategoryID, reading: CategoryReading,
                          tabStop: CGFloat) -> NSAttributedString {
        let value = reading.reading(for: prefs.selection(for: category))
        return twoColumnTitle(left: category.title,
                              right: value.hasData ? value.text : "No data",
                              tabStop: tabStop)
    }

    private func sensorLabel(for sensor: SensorReading) -> String {
        if let key = displayKey(for: sensor) { return "\(key)  \(sensor.label)" }
        return sensor.label
    }

    private func sensorTitle(label: String, key: String?, value: String,
                             tabStop: CGFloat) -> NSAttributedString {
        let left: String
        if let key {
            left = "\(key)  \(label)"
        } else {
            left = label
        }
        return twoColumnTitle(left: left, right: value, tabStop: tabStop)
    }

    /// Right edge of the label column, sized to the longest label in the menu
    /// so rows stay narrow instead of reserving a fixed wide column.
    private func tabStop(for labels: [String]) -> CGFloat {
        let font = NSFont.menuFont(ofSize: 0)
        let widest = labels.reduce(CGFloat(0)) { result, label in
            let width = (label as NSString).size(withAttributes: [.font: font]).width
            return max(result, width)
        }
        return ceil(widest) + columnGap
    }

    private func twoColumnTitle(left: String, right: String, tabStop: CGFloat) -> NSAttributedString {
        let paragraph = NSMutableParagraphStyle()
        paragraph.tabStops = [NSTextTab(textAlignment: .right, location: tabStop, options: [:])]
        paragraph.defaultTabInterval = tabStop

        let result = NSMutableAttributedString()
        result.append(NSAttributedString(string: left + "\t", attributes: [
            .font: NSFont.menuFont(ofSize: 0),
            .paragraphStyle: paragraph,
        ]))
        result.append(NSAttributedString(string: right, attributes: [
            .font: NSFont.monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular),
            .foregroundColor: NSColor.secondaryLabelColor,
            .paragraphStyle: paragraph,
        ]))
        return result
    }

    // MARK: Actions

    @objc private func selectSensor(_ sender: NSMenuItem) {
        guard let selection = sender.representedObject as? SensorSelection else { return }
        prefs.setSelection(selection.sensorID, for: selection.category)

        averageItems[selection.category]?.state = selection.sensorID == nil ? .on : .off
        for (sensorID, item) in sensorItems[selection.category] ?? [:] {
            item.state = sensorID == selection.sensorID ? .on : .off
        }

        // Choosing what a category shows means you want to see it: give it a
        // menu bar slot without making anyone visit Settings.
        if !prefs.menuBarCategories.contains(selection.category) {
            prefs.setMenuBarCategories(prefs.menuBarCategories + [selection.category])
        }

        refreshValues()
        onSelectionChanged?()
    }

    @objc private func intervalChosen(_ sender: NSMenuItem) {
        prefs.setRefreshInterval(Double(sender.tag))
        for item in intervalItems { item.state = item.tag == sender.tag ? .on : .off }
        onIntervalChanged?()
    }

    @objc private func openSettings() {
        onOpenSettings?()
    }

    @objc private func quit() {
        onQuit?()
    }
}
