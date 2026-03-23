# hob

A terminal AI coding agent. Ratatui TUI over a multi-turn agent loop with
tool execution, permission gating, and context compaction.

## Requirements

- Rust toolchain
- An API key from Anthropic or OpenAI

## Installation

```bash
git clone https://github.com/eckertliam/hob.git
cd hob
make build
make install  # copies to ~/.local/bin/
```

## API key setup

Add one of these to your shell profile (`~/.zshrc`, `~/.bashrc`, etc.):

**Anthropic:**

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

**OpenAI:**

```bash
export OPENAI_API_KEY="sk-..."
```

hob auto-detects the provider from whichever key is set. If both are set,
Anthropic is used by default. Override with `export HOB_PROVIDER=openai`.

**OpenAI-compatible APIs** (Ollama, vLLM, etc.):

```bash
export OPENAI_API_KEY="sk-..."
export OPENAI_API_BASE="http://localhost:11434"
export HOB_PROVIDER=openai
```

## Usage

```bash
hob
```

Type your prompt, press `Enter`.

| Key | Action |
|-----|--------|
| `Enter` | Send prompt |
| `Ctrl+J` / `Shift+Enter` | Newline in input |
| `Escape` | Cancel current task |
| `Ctrl-C` | Cancel task / quit |
| `Up` / `Down` | Input history |
| `PageUp` / `PageDown` | Scroll chat |
| `Ctrl+A` / `Ctrl+E` | Beginning / end of input |
| `Ctrl+U` | Clear input |
| `Ctrl+W` | Delete word backwards |
| `Ctrl+L` | Redraw screen |
| `Tab` | Complete slash command |

When a tool needs permission, you'll see a prompt:
`y` = allow once, `!` = allow for session, `n` = deny.

### Slash commands

| Command | Action |
|---------|--------|
| `/model [id]` | List or set the model |
| `/provider anthropic\|openai` | Set provider |
| `/key anthropic\|openai <key>` | Save API key to config |
| `/sessions` | List recent sessions |
| `/clear` | Clear chat history |
| `/help` | Show available commands |

Configuration is saved to `~/.config/hob/config.json`.

## Model selection

Default is `claude-sonnet-4-6`. Set `HOB_MODEL` to override:

```bash
# Anthropic
export HOB_MODEL="claude-sonnet-4-6"          # default
export HOB_MODEL="claude-opus-4-6"            # most capable
export HOB_MODEL="claude-haiku-4-5-20251001"  # fastest

# OpenAI
export HOB_MODEL="gpt-5.4"
export HOB_MODEL="gpt-5.4-mini"
export HOB_MODEL="gpt-5.3-codex"              # coding-optimized
```

## Development

```bash
git clone https://github.com/eckertliam/hob.git
cd hob
cargo run --release --manifest-path agent/Cargo.toml
```

Tests:

```bash
cargo test --manifest-path agent/Cargo.toml
```

## Project instructions

Create a `.hob.md` file in your project root to give the agent context
(coding conventions, test commands, architecture). hob searches upward
from the working directory and includes all `.hob.md` files it finds.

## License

MIT
