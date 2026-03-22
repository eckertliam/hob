EMACS ?= emacs
CARGO ?= cargo
INSTALL_DIR ?= $(HOME)/.local/bin
BINARY_NAME = hob-agent

.PHONY: build install clean byte-compile test test-elisp test-rust test-integration

build:
	touch agent/src/main.rs
	$(CARGO) build --release --manifest-path agent/Cargo.toml

install: build
	cp agent/target/release/$(BINARY_NAME) $(INSTALL_DIR)/

byte-compile:
	$(EMACS) --batch -L lisp/ \
		-f batch-byte-compile lisp/*.el

test: test-rust test-elisp test-integration

test-rust:
	$(CARGO) test --manifest-path agent/Cargo.toml

test-elisp:
	$(EMACS) --batch -L lisp/ -L test/ -l hob-test \
		-f ert-run-tests-batch-and-exit

test-integration: build
	$(EMACS) --batch -L lisp/ -L test/ -l hob-integration-test \
		-f ert-run-tests-batch-and-exit

clean:
	$(CARGO) clean --manifest-path agent/Cargo.toml
	rm -f lisp/*.elc
