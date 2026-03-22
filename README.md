# hob

A native Emacs AI coding agent. Elisp UI drives a Rust subprocess that runs
a multi-turn agent loop over newline-delimited JSON on stdin/stdout.

## Requirements

- Emacs 29.1+
- Rust toolchain (for building the agent binary)
- An Anthropic API key

## Installation

### straight.el

```elisp
(straight-use-package
 '(hob :type git :host github :repo "eckertliam/hob"
       :files ("lisp/*.el")
       :post-build (("make" "build"))))
```

### Manual

```bash
git clone https://github.com/eckertliam/hob.git
cd hob
make build
```

Then add to your Emacs config:

```elisp
(add-to-list 'load-path "/path/to/hob/lisp")
(require 'hob)
```

## API key setup

hob needs an Anthropic API key. Set it one of two ways:

**Environment variable** (recommended):

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**Elisp variable**:

```elisp
(setq hob-api-key "sk-ant-...")
```

If both are set, the Elisp variable takes precedence.

## Usage

```
M-x hob-task    — send a prompt to the agent (starts the subprocess if needed)
M-x hob-start   — start the agent subprocess
M-x hob-stop    — stop the agent subprocess
```

When a tool requires permission (shell commands, file writes), you'll be
prompted: `y` to allow once, `!` to allow for the session, `n` to deny.

## Model selection

The default model is `claude-sonnet-4-20250514`. To change it:

```elisp
(setq hob-model "claude-opus-4-20250514")
```

## Architecture

```
┌──────────────────────────────────────────────────┐
│                 Emacs (lisp/)                    │
│                                                  │
│  hob.el          – entry points, defcustoms      │
│  hob-process.el  – subprocess lifecycle          │
│  hob-ipc.el      – JSON encode/decode, dispatch  │
│  hob-ui.el       – *hob* buffer, permissions UI  │
│                                                  │
│  stdin/stdout: newline-delimited JSON            │
└──────────────────────┬───────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────┐
│              hob-agent (agent/)                  │
│                                                  │
│  agent.rs      – multi-turn tool loop            │
│  api/          – provider abstraction + Anthropic │
│  tools/        – read, write, edit, shell, etc.  │
│  permission.rs – wildcard rules, async ask flow  │
│  compaction.rs – prune + summarize               │
│  error.rs      – classify + retry with backoff   │
│  store.rs      – SQLite session persistence      │
│  prompt.rs     – layered system prompt           │
└──────────────────────────────────────────────────┘
```

The agent loop: call the LLM, stream tokens to Emacs, accumulate tool calls.
If the model wants tools, execute them (with permission checks), feed results
back, and re-prompt. Repeat until the model stops. Context compaction kicks in
automatically when the conversation approaches the model's token limit.

## Project instructions

Create a `.hob.md` file in your project root to give the agent project-specific
context (coding conventions, test commands, architecture notes). hob searches
upward from the working directory and includes all `.hob.md` files it finds.

## Building

```
make build        # cargo build --release
make byte-compile # emacs --batch byte-compile lisp/*.el
make install      # copy binary to ~/.local/bin
make clean
```

## License

MIT
