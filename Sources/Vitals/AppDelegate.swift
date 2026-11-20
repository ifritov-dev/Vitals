import AppKit
import VitalsCore

final class AppDelegate: NSObject, NSApplicationDelegate {
    private let prefs = Preferences()
    private let store = SensorStore()
    private let sensorQueue = DispatchQueue(label: "com.daniel.vitals.sensors")

    private let refreshLock = NSLock()
    private var readInFlight = false
    private var lastReadAt = Date.distantPast

    private var statusController: StatusItemController!
    private var menu: VitalsMenu!
    private var settingsController: PreferencesWindowController?
    private var tickTimer: Timer?
    private var demoTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        store.refresh()

        menu = VitalsMenu(store: store, prefs: prefs)
        statusController = StatusItemController()
        statusController.menuFactory = { [weak self] in
            self?.menu.makeAttachedMenu() ?? NSMenu()
        }
        menu.onSelectionChanged = { [weak self] in self?.applyStatusItems() }
        menu.onIntervalChanged = { [weak self] in self?.restartTimer() }
        menu.onOpenSettings = { [weak self] in self?.showSettings() }
        menu.onQuit = { NSApp.terminate(nil) }

        applyStatusItems()
        restartTimer()

        if CommandLine.arguments.contains("--dump-menu") {
            scheduleMenuDump()
        }
        if CommandLine.arguments.contains("--demo-menu") {
            scheduleDemoMenu()
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    // MARK: Status items

    private func applyStatusItems() {
        statusController.apply(categories: prefs.menuBarCategories, showIcons: prefs.showIcons, compact: prefs.compactSlot)
        statusController.update(readings: store.current(), selections: prefs.selections)
    }

    // MARK: Refresh loop

    private func restartTimer() {
        tickTimer?.invalidate()
        // .common keeps the ticker alive while a menu is tracking, which is
        // what makes the open dropdown update in place.
        let timer = Timer(timeInterval: 0.5, target: self, selector: #selector(tick), userInfo: nil, repeats: true)
        timer.tolerance = 0.1
        RunLoop.main.add(timer, forMode: .common)
        tickTimer = timer
    }

    @objc private func tick() {
        let readings = store.current()
        statusController.update(readings: readings, selections: prefs.selections)
        menu.refreshValues()
        startReadIfDue()
    }

    private func startReadIfDue() {
        refreshLock.lock()
        let due = !readInFlight && Date().timeIntervalSince(lastReadAt) >= prefs.refreshInterval
        if due {
            readInFlight = true
            lastReadAt = Date()
        }
        refreshLock.unlock()
        guard due else { return }

        sensorQueue.async { [weak self] in
            guard let self else { return }
            self.store.refresh()
            self.refreshLock.lock()
            self.readInFlight = false
            self.refreshLock.unlock()
        }
    }

    // MARK: UI

    private func showSettings() {
        if settingsController == nil {
            let controller = PreferencesWindowController(prefs: prefs)
            controller.onCategoriesChanged = { [weak self] in self?.applyStatusItems() }
            controller.onShowIconsChanged = { [weak self] in self?.applyStatusItems() }
            controller.onIntervalChanged = { [weak self] in self?.restartTimer() }
            settingsController = controller
        }
        settingsController?.present()
    }

    // MARK: Headless checks

    private func scheduleMenuDump() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            self.store.refresh()
            let built = self.menu.makeAttachedMenu()
            self.menu.populateForDump(built)
            var text = Diagnostics.menuDump(built, prefs: self.prefs,
                                            frames: self.statusController.snapshotOfFrames)
            text += "\n## Sensor values behind the menu\n"
            text += self.valueTable()
            Output.emit(text)
            NSApp.terminate(nil)
        }
    }

    /// Debug harness: drives the dropdown from a control file so a capture
    /// script can photograph the real menu and its submenus.
    /// Commands (one per line, appended): open, close, move <x> <y>, quit.
    private func scheduleDemoMenu() {
        let path = Output.value(for: "--cmd-file") ?? "/tmp/vitals-cmd.txt"
        try? "".write(toFile: path, atomically: true, encoding: .utf8)
        var handled = 0
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            let watcher = Timer(timeInterval: 0.3, repeats: true) { _ in
                guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { return }
                let commands = text.split(separator: "\n").map(String.init)
                guard commands.count > handled else { return }
                for command in commands[handled...] {
                    handled += 1
                    self.runDemoCommand(command)
                }
            }
            RunLoop.main.add(watcher, forMode: .common)
            self.demoTimer = watcher
        }
    }

    private func runDemoCommand(_ command: String) {
        let parts = command.split(separator: " ").map(String.init)
        switch parts.first {
        case "open":
            guard let button = self.statusController.firstButton else { return }
            self.log("opening dropdown")
            button.performClick(nil)
            self.log("dropdown closed")
        case "submenu":
            // Pops one category's sensor list as a real menu, for capture.
            guard parts.count == 2, let index = Int(parts[1]),
                  let button = self.statusController.firstButton else { return }
            let menu = self.menu.makeAttachedMenu()
            self.menu.populateForDump(menu)
            guard index < menu.items.count, let submenu = menu.items[index].submenu else { return }
            self.log("opening submenu \(index)")
            submenu.popUp(positioning: nil, at: NSPoint(x: 0, y: button.bounds.height + 5), in: button)
            self.log("submenu closed")
        case "close":
            self.menu.openMenu?.cancelTracking()
        case "move":
            guard parts.count == 3, let x = Double(parts[1]), let y = Double(parts[2]) else { return }
            let height = NSScreen.screens.first?.frame.height ?? 982
            CGWarpMouseCursorPosition(CGPoint(x: x, y: height - y))
            self.log("moved cursor to \(x),\(y)")
        case "quit":
            exit(0)
        default:
            break
        }
    }

    private func log(_ message: String) {
        let line = message + "\n"
        let path = Output.value(for: "--geometry-log") ?? "/tmp/vitals-geometry.log"
        if let handle = FileHandle(forWritingAtPath: path) {
            handle.seekToEndOfFile()
            handle.write(line.data(using: .utf8) ?? Data())
            try? handle.close()
        } else {
            try? line.write(toFile: path, atomically: true, encoding: .utf8)
        }
    }

    private func valueTable() -> String {
        var out = ""
        let readings = store.current()
        for category in CategoryID.allCases {
            let reading = readings[category] ?? .empty(category)
            out += "   \(category.title.padding(toLength: 14, withPad: " ", startingAt: 0)) \(reading.average.text)\n"
        }
        return out
    }
}
