# hob

A native Emacs AI coding agent. Elisp UI layer over a Rust subprocess that handles
the agent loop, Anthropic API communication, and tool execution over stdio JSON IPC.

## Architecture

- `lisp/` — Elisp package (UI, buffer management, process lifecycle, IPC)
- `agent/` — Rust binary (agent loop, Anthropic streaming API, tool dispatch)
- Communication: newline-delimited JSON over stdin/stdout

## Building

    make build

## Installing (development)

    make install

## Usage

In Emacs:

    M-x hob-start
