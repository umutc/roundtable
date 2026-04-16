.PHONY: build install uninstall clean

PREFIX ?= $(HOME)/.local

build:
	cargo build --release

install: build
	mkdir -p $(PREFIX)/bin
	cp target/release/roundtable $(PREFIX)/bin/
	cp target/release/roundtable-chat $(PREFIX)/bin/
	@echo "Installed to $(PREFIX)/bin/"
	@echo "Make sure $(PREFIX)/bin is in your PATH"

uninstall:
	rm -f $(PREFIX)/bin/roundtable $(PREFIX)/bin/roundtable-chat

clean:
	cargo clean
