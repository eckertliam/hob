# Provider Abstraction Layer

**Sources**:
- `packages/opencode/src/provider/provider.ts` - Provider registry
- `packages/opencode/src/provider/transform.ts` - Message/schema transforms
- `packages/opencode/src/provider/error.ts` - Error classification
- `packages/opencode/src/provider/models.ts` - Model definitions
- `packages/opencode/src/session/llm.ts` - LLM invocation

## Architecture

```
Session Loop
    │
    ▼
LLM.stream()
    │
    ├─ Provider.getLanguage(providerID, modelID) → LanguageModelV2
    │    Built on Vercel AI SDK (@ai-sdk/provider interface)
    │
    ├─ ProviderTransform.message(msgs, model) → transformed messages
    │    Provider-specific normalization
    │
    ├─ Plugin hooks: chat.params, chat.headers
    │
    └─ ai.streamText({ model, system, messages, tools, ... })
         │
         └─ Returns AsyncIterable of streaming events
```

## Provider Registry

The system supports 30+ LLM providers. Each provider is defined as:

```
Provider.Info = {
  id: ProviderID,        // e.g., "anthropic", "openai", "google"
  name: string,          // Display name
  models: Record<ModelID, Model>,
  options: Record<string, any>,
  env: string[]          // Required environment variables
}
```

### Model Definition

```
Provider.Model = {
  id: ModelID,
  providerID: ProviderID,
  name: string,
  attachment: boolean,      // Supports file attachments?
  reasoning: boolean,       // Has extended thinking?
  family: string,           // e.g., "claude", "gpt", "gemini"
  api: {
    id: string,             // API model identifier
    npm: string,            // AI SDK package name
  },
  limit: {
    context: number,        // Total context window
    input?: number,         // Max input tokens (if different)
    output: number,         // Max output tokens
  },
  cost?: {
    input: number,          // USD per million input tokens
    output: number,         // USD per million output tokens
    cache?: { read, write },
    experimentalOver200K?: { ... }  // Higher rates for large contexts
  },
  capabilities: string[],  // e.g., ["vision", "function_calling"]
}
```

### Provider Loading

Providers use **custom loaders** for provider-specific initialization:

```
CUSTOM_LOADERS = {
  anthropic: (model) => {
    // Add beta headers for interleaved thinking
    // Add fine-grained tool streaming
    return createAnthropic({ headers: betaHeaders })
  },
  openai: (model) => {
    // GPT-5+ uses responses() API instead of chat()
    // Older models use standard chat completion
    return model.useResponses
      ? provider.responses(model.api.id)
      : provider.chat(model.api.id)
  },
  bedrock: (model) => {
    // Region-aware model prefixing (us., eu., global.)
    // AWS credential chain
    return createBedrock({ region })
  },
  // ... etc for Google, Vertex, GitLab, Copilot
}
```

## Message Transformation Pipeline

Before messages reach the LLM, they go through provider-specific transforms:

```
ProviderTransform.message(messages, model):

1. Filter empty messages (Anthropic/Bedrock require non-empty)

2. Sanitize tool call IDs:
   - Claude: alphanumeric + underscores + dashes
   - Mistral: exactly 9 alphanumeric characters
   - Others: pass through

3. Remove empty reasoning/text parts

4. Apply prompt caching markers:
   - First 2 system messages marked for caching
   - Last 2 non-system messages marked for caching
   - Provider-specific format:
     - Anthropic: cacheControl: { type: "ephemeral" }
     - Bedrock: cachePoint: { type: "default" }
     - OpenAI/OpenRouter: cache_control: { type: "ephemeral" }
```

## Streaming Implementation

All providers use Server-Sent Events (SSE) for streaming:

```
LLM.stream() → streamText() from "ai" package

The stream yields events:
  "start"
  "text-start" / "text-delta" / "text-end"
  "reasoning-start" / "reasoning-delta" / "reasoning-end"
  "tool-input-start" / "tool-input-delta" / "tool-input-end"
  "tool-call"
  "tool-result" / "tool-error"
  "start-step" / "finish-step"
  "finish"
  "error"
```

### Tool Call Streaming

Tool calls arrive incrementally:

```
1. tool-input-start: { id, toolName }
   → Create pending tool part

2. tool-input-delta: { id, argsTextDelta }
   → Accumulate argument JSON string
   → Test parsability at each chunk (isParsableJson)

3. tool-call: { id, toolName, args }
   → Full args now available
   → Execute tool
   → Return result

4. tool-result: { id, result }
   → Tool execution complete
```

Multiple tool calls can stream concurrently, tracked by index/ID.

## Token Counting and Cost

### Output Token Limits
```
OUTPUT_TOKEN_MAX = 32,000 (default cap)
maxOutputTokens = min(model.limit.output, OUTPUT_TOKEN_MAX)
```

### Reasoning Token Budgets
```
Claude (adaptive):  budgetTokens = min(31,999, model.limit.output - 1)
Google Gemini:      thinkingBudget = varies by verbosity setting
OpenAI GPT-5:      reasoningEffort = "low" | "medium" | "high"
```

### Token Usage Normalization

Different providers report tokens differently:

```
Anthropic/Bedrock:
  inputTokens EXCLUDES cached tokens
  Must manually add: total = input + output + cache_read + cache_write

OpenAI/Others:
  inputTokens INCLUDES cached tokens
  Must subtract: adjustedInput = input - cache_read - cache_write
```

### Cost Calculation
```
cost = (adjustedInput × input_rate / 1M)
     + (output × output_rate / 1M)
     + (cache_read × cache_read_rate / 1M)
     + (cache_write × cache_write_rate / 1M)
     + (reasoning × output_rate / 1M)
```

Special: if `input + cache_read > 200K`, use higher pricing tier.

## Error Classification

The error module classifies provider errors using regex patterns:

```
Context Overflow:
  Anthropic: "prompt is too long"
  OpenAI: "exceeds the context window"
  Google: "input token count.*exceeds the maximum"
  Generic: "context.*length.*exceeded", "token limit"

Rate Limit:
  HTTP 429, "rate limit", "too many requests"

Server Error:
  HTTP 500-599, "overloaded", "internal server error"

Auth Error:
  HTTP 401/403, "authentication", "unauthorized"
```

Each is mapped to a `ParsedAPICallError` with a type field used by the
retry logic.

## Prompt Caching

The harness optimizes API costs by marking messages for caching:

```
Strategy:
  Mark first 2 system messages for caching (stable content)
  Mark last 2 non-system messages for caching (recent context)

Provider-specific markers:
  Anthropic: providerOptions.anthropic.cacheControl = { type: "ephemeral" }
  Bedrock:   providerOptions.bedrock.cachePoint = { type: "default" }
  OpenAI:    providerOptions.openai.cache_control = { type: "ephemeral" }
```

This takes advantage of provider-side prompt caching to reduce cost and
latency for repeated content.

## System Prompt Assembly in LLM.stream()

```
system = []

if agent.prompt exists:
  system = [agent.prompt]
else:
  system = [providerPrompt(model)]  // beast.txt, anthropic.txt, etc.

system.push(...input.system)         // Environment, skills, instructions
system.push(...user.system)          // User-specific system prompt

// Plugin hook: may modify system array
Plugin.trigger("experimental.chat.system.transform", { model }, { system })

// For OpenAI OAuth: passed as "instructions" parameter (not system messages)
// For others: prepended as system role messages
```

## Design Insights for Emacs

1. **Vercel AI SDK as the abstraction**: The `LanguageModelV2` interface
   provides a uniform streaming API across 30+ providers. Your Emacs
   implementation needs a similar abstraction -- probably a generic
   "streaming chat completion" interface with provider-specific backends.

2. **Transform pipeline is essential**: Every provider has quirks. Tool call
   ID formats, empty message handling, cache markers, token counting.
   Plan for a normalization layer.

3. **Prompt caching saves money**: Mark stable content (system prompt,
   instructions) for caching. Mark recent context for caching. This can
   reduce costs 50-90% for long sessions.

4. **Error classification drives retry**: Don't retry auth errors. Do retry
   rate limits with backoff. Trigger compaction on context overflow. The
   error type determines the strategy.

5. **Reasoning tokens need special handling**: Extended thinking (Claude),
   reasoning effort (OpenAI) -- different APIs, different budget controls,
   but same concept. Abstract as "thinking configuration" per model.
