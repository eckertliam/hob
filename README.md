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

Set one:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
# or
export OPENAI_API_KEY="sk-..."
```

Auto-detects provider from whichever key is set. Force with `HOB_PROVIDER=openai` or `HOB_PROVIDER=anthropic`.

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
| `Ctrl-C` | Cancel task / quit |
| `Up` / `Down` | Input history |
| `PageUp` / `PageDown` | Scroll chat |

When a tool needs permission, you'll see a prompt:
`y` = allow once, `!` = allow for session, `n` = deny.

## Model selection

```bash
export HOB_MODEL="claude-sonnet-4-20250514"  # default
export HOB_MODEL="gpt-4o"
```

## Project instructions

Create a `.hob.md` file in your project root to give the agent context
(coding conventions, test commands, architecture). hob searches upward
from the working directory and includes all `.hob.md` files it finds.

## License

MIT
