APP_NAME := rune
VERSION  := 0.1.0

.PHONY: build install clean app-macos clean-app

build:
	cargo build --release

install: build
	install -d $(DESTDIR)/usr/local/bin
	install -m 0755 target/release/$(APP_NAME) $(DESTDIR)/usr/local/bin/$(APP_NAME)

clean:
	cargo clean
	rm -rf dist

# Create a simple macOS .app that opens Terminal and runs vedit
app-macos: build
	rm -rf dist/$(APP_NAME).app
	mkdir -p dist/$(APP_NAME).app/Contents/MacOS
	cp target/release/$(APP_NAME) dist/$(APP_NAME).app/Contents/MacOS/$(APP_NAME)
	cp packaging/macos/Info.plist dist/$(APP_NAME).app/Contents/Info.plist
	cp packaging/macos/rune-launcher dist/$(APP_NAME).app/Contents/MacOS/rune-launcher
	chmod +x dist/$(APP_NAME).app/Contents/MacOS/rune-launcher

clean-app:
	rm -rf dist/$(APP_NAME).app
