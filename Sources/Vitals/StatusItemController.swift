import AppKit
import VitalsCore

/// Owns the menu bar slots.
///
/// In compact mode every configured category shares one slot and reads as a
/// single tight run ("29% 4.53 63%"), which is what the GNOME Vitals panel
/// looks like. Otherwise each category gets its own slot.
final class StatusItemController {
    private var items: [CategoryID: NSStatusItem] = [:]
    private var compactItem: NSStatusItem?
    private var orderedCategories: [CategoryID] = []
    private var showIcons = true
    private var compact = true

    /// Supplies a menu instance for a status item.
    var menuFactory: (() -> NSMenu)?

    func apply(categories: [CategoryID], showIcons: Bool, compact: Bool) {
        self.showIcons = showIcons
        let needsRebuild = categories != orderedCategories || compact != self.compact
        self.compact = compact
        guard needsRebuild else {
            refreshImages()
            return
        }

        for (_, item) in items { NSStatusBar.system.removeStatusItem(item) }
        items = [:]
        if let compactItem { NSStatusBar.system.removeStatusItem(compactItem) }
        compactItem = nil
        orderedCategories = categories

        if compact {
            let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
            if let button = item.button {
                button.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
                button.attributedTitle = NSAttributedString(string: "Vitals", attributes: [.font: button.font!])
                item.menu = menuFactory?()
            }
            compactItem = item
        } else {
            for category in categories {
                items[category] = makeItem(for: category)
            }
        }
    }

    private func makeItem(for category: CategoryID) -> NSStatusItem {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = item.button {
            button.font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
            button.attributedTitle = NSAttributedString(string: "—", attributes: [.font: button.font!])
            button.toolTip = "Vitals — \(category.title)"
            item.menu = menuFactory?()
        }
        return item
    }

    private func refreshImages() {
        for category in orderedCategories {
            items[category]?.button?.needsDisplay = true
        }
        compactItem?.button?.needsDisplay = true
    }

    func update(readings: [CategoryID: CategoryReading], selections: [CategoryID: String]) {
        var runs: [NSAttributedString] = []
        for category in orderedCategories {
            let reading = readings[category]?.reading(for: selections[category]) ?? .noData
            let value = reading.hasData ? reading.compact : "—"
            guard let symbol = symbol(for: category, reading: reading) else {
                runs.append(NSAttributedString(string: value))
                continue
            }
            runs.append(slotContent(symbol: symbol, value: value))
        }

        if compact {
            guard let button = compactItem?.button else { return }
            let combined = NSMutableAttributedString()
            let font = button.font ?? NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
            for (index, run) in runs.enumerated() {
                if index > 0 {
                    combined.append(NSAttributedString(string: "  ", attributes: [.font: font]))
                }
                combined.append(run)
            }
            if combined.length == 0 {
                combined.append(NSAttributedString(string: "Vitals", attributes: [.font: font]))
            }
            if button.attributedTitle.string != combined.string {
                button.attributedTitle = combined
            }
            compactItem?.button?.toolTip = orderedCategories
                .map { category in
                    let reading = readings[category]?.reading(for: selections[category]) ?? .noData
                    return "\(category.title): \(reading.hasData ? reading.text : "No data")"
                }
                .joined(separator: "\n")
            return
        }

        for (index, category) in orderedCategories.enumerated() {
            guard let button = items[category]?.button, index < runs.count else { continue }
            if button.attributedTitle.string != runs[index].string {
                button.attributedTitle = runs[index]
            }
        }
    }

    /// Icon plus value as one run: attachment, thin space, number.
    private func slotContent(symbol: NSImage, value: String) -> NSAttributedString {
        let font = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        guard showIcons else {
            return NSAttributedString(string: value, attributes: [.font: font])
        }
        let result = NSMutableAttributedString()
        let attachment = NSTextAttachment()
        attachment.image = symbol
        attachment.bounds = CGRect(x: 0, y: -2.5, width: 14, height: 14)
        result.append(NSAttributedString(attachment: attachment))
        result.append(NSAttributedString(string: "\u{2009}" + value, attributes: [.font: font]))
        return result
    }

    private func symbol(for category: CategoryID, reading: ReadingValue) -> NSImage? {
        if category == .battery { return Icons.battery(for: reading.value) }
        return Icons.symbol(for: category)
    }

    var firstButton: NSStatusBarButton? {
        if compact { return compactItem?.button }
        return orderedCategories.compactMap { items[$0]?.button }.first
    }

    var snapshotOfFrames: [(String, NSRect?, Bool)] {
        if compact {
            return [("compact slot", compactItem?.button?.window?.frame, compactItem?.isVisible ?? false)]
        }
        return orderedCategories.map { category in
            ("Vitals \(category.title)", items[category]?.button?.window?.frame, items[category]?.isVisible ?? false)
        }
    }
}

enum Icons {
    static func symbol(for category: CategoryID) -> NSImage? {
        let configuration = NSImage.SymbolConfiguration(pointSize: 12, weight: .medium)
        for name in category.symbolCandidates {
            if let image = NSImage(systemSymbolName: name, accessibilityDescription: category.title)?
                .withSymbolConfiguration(configuration) {
                image.isTemplate = true
                return image
            }
        }
        return nil
    }

    static func symbolNamed(_ name: String, pointSize: CGFloat = 13) -> NSImage? {
        guard let image = NSImage(systemSymbolName: name, accessibilityDescription: nil)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: pointSize, weight: .regular))
        else { return nil }
        image.isTemplate = true
        return image
    }

    static func battery(for percent: Double) -> NSImage? {
        let name: String
        switch percent {
        case 87.5...: name = "battery.100"
        case 62.5..<87.5: name = "battery.75"
        case 37.5..<62.5: name = "battery.50"
        case 12.5..<37.5: name = "battery.25"
        default: name = "battery.0"
        }
        return symbolNamed(name, pointSize: 12) ?? symbolNamed("battery.100", pointSize: 12)
    }
}
