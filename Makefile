EMACS ?= emacs
CARGO ?= cargo
INSTALL_DIR ?= $(HOME)/.local/bin
BINARY_NAME = hob-agent

.PHONY: build install clean byte-compile

build:
	$(CARGO) build --release --manifest-path agent/Cargo.toml

install: build
	cp agent/target/release/$(BINARY_NAME) $(INSTALL_DIR)/

byte-compile:
	$(EMACS) --batch -L lisp/ \
		-f batch-byte-compile lisp/*.el

clean:
	$(CARGO) clean --manifest-path agent/Cargo.toml
	rm -f lisp/*.elc
