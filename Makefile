# The rustup toolchain may not be on PATH (especially when make runs from a
# non-interactive shell), so cargo and rustc are both resolved from it.
TOOLCHAIN   ?= $(HOME)/.rustup/toolchains/stable-aarch64-apple-darwin/bin
CARGO       ?= $(shell command -v cargo 2>/dev/null || echo $(TOOLCHAIN)/cargo)
BUILD_ENV   := PATH="$(TOOLCHAIN):$$PATH"

VERSION     := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP_NAME    := Vitals
BUNDLE      := dist/$(APP_NAME).app
DMG         := dist/$(APP_NAME)-$(VERSION).dmg
RELEASE_BIN := target/release/vitals
INSTALL_DIR := /Applications

.PHONY: build app run install uninstall dmg dump verify clean

build:
	$(BUILD_ENV) $(CARGO) build --release

app: build
	rm -rf $(BUNDLE)
	mkdir -p $(BUNDLE)/Contents/MacOS $(BUNDLE)/Contents/Resources
	@touch dist/.metadata_never_index
	cp $(RELEASE_BIN) $(BUNDLE)/Contents/MacOS/$(APP_NAME)
	cp packaging/Info.plist $(BUNDLE)/Contents/Info.plist
	cp packaging/Phosphor.ttf $(BUNDLE)/Contents/Resources/Phosphor.ttf
	cp packaging/AppIcon.icns $(BUNDLE)/Contents/Resources/AppIcon.icns
	printf 'APPL????' > $(BUNDLE)/Contents/PkgInfo
	/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $(VERSION)" \
		-c "Set :CFBundleVersion $(VERSION)" $(BUNDLE)/Contents/Info.plist
	codesign --force --sign - --timestamp=none $(BUNDLE)
	@codesign --verify --strict $(BUNDLE) && echo "✔ signed and ready: $(BUNDLE)"

run: app
	open $(BUNDLE)

install: app
	rm -rf $(INSTALL_DIR)/$(APP_NAME).app
	cp -R $(BUNDLE) $(INSTALL_DIR)/$(APP_NAME).app
	rm -rf $(BUNDLE) dist/.metadata_never_index
	@for stray in $(INSTALL_DIR)/$(APP_NAME)*.app; do \
		case "$$stray" in "$(INSTALL_DIR)/$(APP_NAME).app") continue;; esac; \
		if [ -e "$$stray" ]; then \
			rm -rf "$$stray" && echo "removed stray $$stray" || exit 1; \
		fi; \
	done
	@echo "✔ installed $(INSTALL_DIR)/$(APP_NAME).app (the only copy)"

dmg: app
	@rm -rf dist/dmgroot && mkdir -p dist/dmgroot
	cp -R $(BUNDLE) dist/dmgroot/
	ln -s /Applications dist/dmgroot/Applications
	hdiutil create -quiet -volname "$(APP_NAME)" -srcfolder dist/dmgroot \
		-fs HFS+ -format UDZO -ov $(DMG)
	@rm -rf dist/dmgroot
	@echo "✔ $(DMG)"

uninstall:
	@rm -rf $(INSTALL_DIR)/$(APP_NAME).app && echo "✔ removed $(INSTALL_DIR)/$(APP_NAME).app"

verify: app
	$(BUNDLE)/Contents/MacOS/$(APP_NAME) --selftest
	$(BUNDLE)/Contents/MacOS/$(APP_NAME) --dump --out /tmp/vitals-dump.txt

dump: build
	$(RELEASE_BIN) --dump

clean:
	$(BUILD_ENV) $(CARGO) clean && rm -rf dist
