import AppKit
import ServiceManagement
import VitalsCore

/// The Settings window: which categories sit in the menu bar, how often to
/// refresh, and whether slots carry an icon.
final class PreferencesWindowController: NSWindowController {
    private let prefs: Preferences
    private var checkboxes: [CategoryID: NSButton] = [:]
    private var showIconsButton: NSButton?
    private var compactButton: NSButton?
    private var launchAtLoginButton: NSButton?
    private var intervalPopUp: NSPopUpButton?

    var onCategoriesChanged: (() -> Void)?
    var onIntervalChanged: (() -> Void)?
    var onShowIconsChanged: (() -> Void)?

    init(prefs: Preferences) {
        self.prefs = prefs
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 380, height: 560),
                              styleMask: [.titled, .closable],
                              backing: .buffered,
                              defer: false)
        window.title = "Vitals Settings"
        window.isReleasedWhenClosed = false
        window.center()
        super.init(window: window)
        window.contentView = buildContent()
    }

    required init?(coder: NSCoder) { fatalError("not supported") }

    func present() {
        NSApp.activate(ignoringOtherApps: true)
        showWindow(nil)
        window?.makeKeyAndOrderFront(nil)
    }

    private func buildContent() -> NSView {
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false

        stack.addArrangedSubview(header("Menu Bar"))
        stack.addArrangedSubview(caption("Categories shown as slots in the menu bar."))
        for category in CategoryID.allCases {
            let button = NSButton(checkboxWithTitle: category.title,
                                  target: self,
                                  action: #selector(categoryToggled))
            button.state = prefs.menuBarCategories.contains(category) ? .on : .off
            checkboxes[category] = button
            stack.addArrangedSubview(button)
        }

        let compact = NSButton(checkboxWithTitle: "Single compact slot (all values in one run)",
                               target: self, action: #selector(compactToggled))
        compact.state = prefs.compactSlot ? .on : .off
        compactButton = compact
        stack.addArrangedSubview(spacer(4))
        stack.addArrangedSubview(compact)

        let icons = NSButton(checkboxWithTitle: "Show icons in the menu bar",
                             target: self, action: #selector(showIconsToggled))
        icons.state = prefs.showIcons ? .on : .off
        showIconsButton = icons
        stack.addArrangedSubview(icons)

        stack.addArrangedSubview(spacer(10))
        stack.addArrangedSubview(header("Refresh"))
        let intervalRow = NSStackView()
        intervalRow.orientation = .horizontal
        intervalRow.spacing = 8
        intervalRow.addArrangedSubview(caption("Update every"))
        let popUp = NSPopUpButton(frame: .zero, pullsDown: false)
        for seconds in 1...10 {
            popUp.addItem(withTitle: "\(seconds) s")
            popUp.lastItem?.tag = seconds
        }
        popUp.selectItem(withTag: Int(prefs.refreshInterval))
        popUp.target = self
        popUp.action = #selector(intervalChanged)
        intervalPopUp = popUp
        intervalRow.addArrangedSubview(popUp)
        stack.addArrangedSubview(intervalRow)

        stack.addArrangedSubview(spacer(10))
        stack.addArrangedSubview(header("General"))
        let login = NSButton(checkboxWithTitle: "Launch at login",
                             target: self, action: #selector(launchAtLoginToggled))
        login.state = SMAppService.mainApp.status == .enabled ? .on : .off
        launchAtLoginButton = login
        stack.addArrangedSubview(login)

        guard let content = window?.contentView else { return stack }
        content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 20),
            stack.trailingAnchor.constraint(lessThanOrEqualTo: content.trailingAnchor, constant: -20),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 20),
            stack.bottomAnchor.constraint(lessThanOrEqualTo: content.bottomAnchor, constant: -20),
        ])
        return content
    }

    private func header(_ text: String) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = NSFont.systemFont(ofSize: 13, weight: .semibold)
        return label
    }

    private func caption(_ text: String) -> NSTextField {
        let label = NSTextField(labelWithString: text)
        label.font = NSFont.systemFont(ofSize: 11)
        label.textColor = .secondaryLabelColor
        return label
    }

    private func spacer(_ height: CGFloat) -> NSView {
        let view = NSView()
        view.translatesAutoresizingMaskIntoConstraints = false
        view.heightAnchor.constraint(equalToConstant: height).isActive = true
        return view
    }

    @objc private func categoryToggled() {
        let selected = CategoryID.allCases.filter { checkboxes[$0]?.state == .on }
        prefs.setMenuBarCategories(selected)
        onCategoriesChanged?()
    }

    @objc private func compactToggled() {
        prefs.setCompactSlot(compactButton?.state == .on)
        onCategoriesChanged?()
    }

    @objc private func showIconsToggled() {
        prefs.setShowIcons(showIconsButton?.state == .on)
        onShowIconsChanged?()
    }

    @objc private func intervalChanged() {
        if let tag = intervalPopUp?.selectedItem?.tag {
            prefs.setRefreshInterval(Double(tag))
            onIntervalChanged?()
        }
    }

    @objc private func launchAtLoginToggled() {
        let service = SMAppService.mainApp
        do {
            if launchAtLoginButton?.state == .on {
                try service.register()
            } else {
                try service.unregister()
            }
        } catch {
            launchAtLoginButton?.state = service.status == .enabled ? .on : .off
            launchAtLoginButton?.toolTip = "Could not change the login item: \(error.localizedDescription)"
        }
    }
}
