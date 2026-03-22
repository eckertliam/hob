CARGO ?= cargo
INSTALL_DIR ?= $(HOME)/.local/bin

.PHONY: build install clean test

build:
	touch agent/src/main.rs
	$(CARGO) build --release --manifest-path agent/Cargo.toml

install: build
	cp agent/target/release/hob $(INSTALL_DIR)/

test:
	$(CARGO) test --manifest-path agent/Cargo.toml

clean:
	$(CARGO) clean --manifest-path agent/Cargo.toml
