# The rustup toolchain may not be on PATH (especially when make runs from a
# non-interactive shell), so cargo and rustc are both resolved from it.
TOOLCHAIN   ?= $(HOME)/.rustup/toolchains/stable-aarch64-apple-darwin/bin
CARGO       ?= $(shell command -v cargo 2>/dev/null || echo $(TOOLCHAIN)/cargo)
BUILD_ENV   := PATH="$(TOOLCHAIN):$$PATH"

APP_NAME    := Vitals
BUNDLE      := dist/$(APP_NAME).app
RELEASE_BIN := target/release/vitals
INSTALL_DIR := /Applications

.PHONY: build app run install uninstall dump verify clean

build:
	$(BUILD_ENV) $(CARGO) build --release

app: build
	rm -rf $(BUNDLE)
	mkdir -p $(BUNDLE)/Contents/MacOS $(BUNDLE)/Contents/Resources
	cp $(RELEASE_BIN) $(BUNDLE)/Contents/MacOS/$(APP_NAME)
	cp packaging/Info.plist $(BUNDLE)/Contents/Info.plist
	printf 'APPL????' > $(BUNDLE)/Contents/PkgInfo
	codesign --force --sign - --timestamp=none $(BUNDLE)
	@codesign --verify --strict $(BUNDLE) && echo "✔ signed and ready: $(BUNDLE)"

run: app
	open $(BUNDLE)

install: app
	@if [ -e $(INSTALL_DIR)/$(APP_NAME).app ]; then \
		echo "✖ $(INSTALL_DIR)/$(APP_NAME).app already exists — refusing to overwrite"; exit 1; \
	fi
	cp -R $(BUNDLE) $(INSTALL_DIR)/$(APP_NAME).app
	@echo "✔ installed $(INSTALL_DIR)/$(APP_NAME).app"

uninstall:
	@rm -rf $(INSTALL_DIR)/$(APP_NAME).app && echo "✔ removed $(INSTALL_DIR)/$(APP_NAME).app"

verify: app
	$(BUNDLE)/Contents/MacOS/$(APP_NAME) --selftest
	$(BUNDLE)/Contents/MacOS/$(APP_NAME) --dump --out /tmp/vitals-dump.txt

dump: build
	$(RELEASE_BIN) --dump

clean:
	$(BUILD_ENV) $(CARGO) clean && rm -rf dist
