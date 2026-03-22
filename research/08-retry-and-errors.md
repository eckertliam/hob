# Retry and Error Handling

**Source**: `packages/opencode/src/session/retry.ts`, `packages/opencode/src/provider/error.ts`

## Error Classification

When the LLM API returns an error, the system classifies it:

```
Error received
    │
    ├─ ContextOverflowError?
    │   Detected by regex patterns per provider:
    │     Anthropic: "prompt is too long"
    │     OpenAI: "exceeds the context window"
    │     Google: "input token count.*exceeds the maximum"
    │     Generic: "context.*length.*exceeded"
    │   → Never retried. Triggers compaction.
    │
    ├─ Auth Error (401/403)?
    │   → Never retried.
    │
    ├─ Free Usage Limit Error?
    │   → Never retried.
    │
    ├─ Rate Limit (429, "rate_limit", "too_many_requests")?
    │   → Retryable: "Rate Limited"
    │
    ├─ Overloaded (503, "exhausted", "unavailable", "overloaded")?
    │   → Retryable: "Provider is overloaded"
    │
    ├─ Other API Error with isRetryable: true?
    │   → Retryable: error message
    │
    └─ Other API Error with isRetryable: false?
        → Not retried.
```

## Exponential Backoff Algorithm

```
Constants:
  INITIAL_DELAY    = 2,000 ms (2 seconds)
  BACKOFF_FACTOR   = 2
  MAX_DELAY_NO_HDR = 30,000 ms (30 seconds, without Retry-After headers)
  MAX_DELAY        = 2,147,483,647 ms (with Retry-After headers)

delay(attempt, error):
  1. Check error for Retry-After headers:
     a. "retry-after-ms" header → parse as float ms
     b. "retry-after" header → parse as seconds (× 1000) or HTTP date
     c. If valid header found → return min(header_value, MAX_DELAY)

  2. If no valid header:
     return min(INITIAL_DELAY × 2^(attempt-1), MAX_DELAY_NO_HDR)
```

**Delay sequence without headers:**
```
Attempt 1:  2,000 ms (2s)
Attempt 2:  4,000 ms (4s)
Attempt 3:  8,000 ms (8s)
Attempt 4: 16,000 ms (16s)
Attempt 5: 30,000 ms (30s, capped)
Attempt 6: 30,000 ms (capped)
...
```

## No Max Retry Limit

There is **no hardcoded maximum** number of retries. The system retries
indefinitely as long as:
- The error remains retryable
- The abort signal hasn't fired
- No non-retryable error occurs

The only practical limits are:
- User cancels (abort signal)
- Error becomes non-retryable
- The delay between retries (caps at 30s without headers)

## Retry-After Header Handling

The system respects standard HTTP retry headers:

```
Priority:
1. "retry-after-ms" (custom header, milliseconds as float)
2. "retry-after" (standard, seconds as decimal)
3. "retry-after" (standard, HTTP-date format)
4. Exponential backoff fallback

With headers: delay can be very large (up to ~24 days)
Without headers: capped at 30 seconds
```

## Session Status During Retries

```
Status transitions:

  "busy" → (retryable error) → "retry" { attempt, message, next }
    │                              │
    │                              ├─ sleep(delay, abort)
    │                              │
    │                              └─ back to "busy" (loop continues)
    │
    └─ (non-retryable error) → "idle"
```

The "retry" status includes:
- `attempt`: Current attempt number
- `message`: User-facing reason (e.g., "Rate Limited")
- `next`: Unix timestamp when next retry occurs

This allows the UI to show a countdown and reason to the user.

## Cancellable Sleep

```
SessionRetry.sleep(ms, abortSignal):
  Returns a Promise that:
  - Resolves after ms milliseconds
  - Rejects immediately if abortSignal fires

  Implementation:
  - Sets up setTimeout(resolve, ms)
  - Adds abort event listener that:
    1. Clears the timeout
    2. Rejects with AbortError

  In processor.ts: rejection is caught and suppressed
  → allows the loop to check abort and break gracefully
```

## Error Type Hierarchy

```
MessageV2.fromError(e, { providerID }):
  Classifies errors into named types:

  ContextOverflowError  → triggers compaction
  APIError              → may be retryable (checked by retryable())
  FreeUsageLimitError   → never retried
  AuthError             → never retried
  AbortedError          → user cancelled
  StructuredOutputError → schema mismatch
  NamedError.Unknown    → fallback

  Error metadata includes:
    name, message, stack trace, provider-specific details
```

## Design Insights for Emacs

1. **Infinite retry with backoff is the right default**: Rate limits and
   overloads are transient. Just wait and retry. The user can always cancel.

2. **Retry-After headers matter**: Providers tell you exactly when to retry.
   Respect that. Saves unnecessary 429s.

3. **Session status as retry feedback**: Show the user what's happening
   ("Rate limited, retrying in 8s") instead of just spinning.

4. **Context overflow → compaction, not retry**: An overflow error means
   the input is too large. Retrying won't help. Summarize and retry with
   shorter input.

5. **Cancellable sleep is important**: Don't trap the user in a 30-second
   wait. Make retry delays interruptible via abort signal.
