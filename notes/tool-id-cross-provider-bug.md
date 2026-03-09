# Bug: OpenAI-format tool IDs break claude-max on fallback

**Date:** 2026-03-09  
**Status:** Investigated, fix planned on branch `fix/normalize-tool-ids-on-fallback`  
**Symptom:** Repeated 400 errors from claude-max:

```
⚠️ Provider claude-max streaming failed: Provider API error: 400 -
claude-max API error: {"type":"error","error":{"type":"invalid_request_error",
"message":"messages.39: `tool_use` ids were found without `tool_result` blocks
immediately after: call_4CfDBgdmPHewfyQbjnSeqY7x, call_lvEHMtRqDVPJKgx3ZI2BuP48.
Each `tool_use` block must have a corresponding `tool_result` block in the
next message."}}
```

---

## Root cause

### Trigger

Claude-max's OAuth token started failing with `invalid_grant` around 15:46 local
time on 2026-03-09. Simultaneously, chatgpt-plus was rate-limited (429).
Both primary and secondary providers for the `claude-sonnet-4-6` model were
unavailable, so CCM fell through to `opencode-zen` (priority 3–7 depending on
model), which is an OpenAI-compatible endpoint.

### The failure chain

1. **Turn N:** CCM tries `claude-max` → 401 (OAuth). Tries `chatgpt-plus` → 429.
   Falls through to `opencode-zen` (provider_type = "openai"). Request succeeds.

2. **opencode-zen returns tool calls** with OpenAI-format `call_xxx` IDs
   (e.g. `call_4CfDBgdmPHewfyQbjnSeqY7x`). CCM's OpenAI provider streams
   these back to Claude Code as Anthropic SSE – the `call_xxx` strings are
   written verbatim into `content_block_start` events for `tool_use` blocks.

3. **Claude Code runs the tools.** It sends back a user message containing
   `tool_result` blocks referencing the same `call_xxx` IDs. The conversation
   history now contains an assistant turn with `tool_use id="call_xxx"` and a
   user turn with `tool_result tool_use_id="call_xxx"`.

4. **Turn N+1:** CCM tries `claude-max` again (priority 1, token may have
   refreshed). It forwards the full conversation history.

5. **Anthropic rejects the request with 400.** The history is structurally
   broken from Anthropic's perspective: assistant message at index 39 (or 225,
   or 3 in other sessions) has `tool_use` blocks whose IDs are not followed by
   matching `tool_result` blocks in the very next message. Anthropic enforces a
   strict structural invariant that every `tool_use` in an assistant message must
   have a corresponding `tool_result` immediately after.

   The mismatch is **not** about ID format (the `call_xxx` characters are all
   legal); it is about the structural pairing in the message array. When CCM
   reconstructs the Anthropic message list from the raw request JSON (which was
   originally constructed by Claude Code against the Anthropic API contract), the
   `tool_result` blocks that match those IDs end up in a different position than
   Anthropic expects — because Claude Code's internal state machine tracks IDs by
   value, and it did emit the correct results, but the CCM message-reconstruction
   path for mixed-provider history loses the pairing.

6. **The loop persists.** Claude Code retries the same turn each time it gets
   a new user message. The broken history is permanent until Claude Code's
   context is cleared. All three stuck conversations visible in the log (with
   distinct tool-ID sets) started at the same time, confirming the trigger was
   the simultaneous claude-max OAuth failure + chatgpt-plus rate-limit.

### Why `sanitize_tool_use_ids` doesn't fix this

`sanitize_tool_use_ids` in `anthropic_compatible.rs` replaces non-`[a-zA-Z0-9_-]`
characters in tool IDs. The `call_xxx` IDs already use only legal characters,
so they pass through unchanged. The sanitizer addresses a different problem
(IDs from providers that emit e.g. spaces or dots in IDs).

### Three distinct stuck conversations (from logs)

| Session | Tool IDs                                                                                                                                                                                                                              | Message index in error |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- |
| A       | `call_4CfDBgdmPHewfyQbjnSeqY7x`, `call_lvEHMtRqDVPJKgx3ZI2BuP48`                                                                                                                                                                      | 39                     |
| B       | `call_6EyZdy4ck9lZZmbxQJIuFhEm`, `call_B96uC8UJ3Cdo2APzjcy5axDc`, `call_wVKVqBhEV9jnIEYXkdP5l0vJ`                                                                                                                                     | 225                    |
| C       | `call_8FUxdbKcOTBhMGBjyle5An6t`, `call_9Cet5gu8wNKaPYM1JBeTE8s4`, `call_PXNoXcLrulDz9P6hg7DUsnPb`, `call_RD6y1vhalJZ9C3GLjY327Lp8`, `call_jkyhRezk6FXAxpVs68M0OqK2`, `call_lMncZIEqBw635pvDFy8DGk0Y`, `call_qFaAwpspmZipDZcNv2S9F1cU` | 3                      |

All three appeared together at ~15:48 local time and have been repeating ever
since, confirming they are stuck Claude Code sessions replaying the same
broken context on every new user turn.

---

## Options considered

### Option A – Skip Anthropic providers when history contains `call_xxx` IDs

When about to forward a request to an Anthropic provider and the message history
contains any `tool_use` block whose ID starts with `call_`, skip that provider
and try the next fallback.

**Pros:** Simple to implement. Zero risk of corrupting valid data.  
**Cons:** Permanently blacklists Anthropic for the lifetime of a conversation
once any `call_xxx` ID appears, even if the history could be repaired. Does not
fix already-stuck sessions.

### Option B – Normalize tool IDs to Anthropic format before forwarding ✓ CHOSEN

When the target provider is Anthropic and the request contains `tool_use` blocks
with non-Anthropic IDs (detected by absence of `toolu_` prefix), rewrite both
the `tool_use.id` fields in assistant messages **and** the matching
`tool_result.tool_use_id` fields in user messages to synthesized `toolu_`-prefixed
IDs, maintaining the pairing.

**Pros:** The conversation continues working normally with claude-max. Already-stuck
sessions get unblocked on the next user message.  
**Cons:** Slightly more complex than Option A. Requires careful paired rewriting
to avoid mismatches.

### Option C – Fix the OAuth token (immediate mitigation)

Reauthenticate claude-max so it stops failing. If the primary provider never
fails, the fallback chain never reaches opencode-zen, `call_xxx` IDs never enter
the history, and this bug doesn't fire.

**Pros:** Immediate, zero code change.  
**Cons:** Does not fix the underlying architectural issue. Any future primary
provider failure will re-trigger the bug.

---

## Decision

**Implement Option B** with Option C as immediate mitigation.

Option B is the correct long-term fix: CCM should be able to use any provider
in any position in the fallback chain without poisoning subsequent Anthropic
requests. The history normalization happens transparently at the Anthropic
provider boundary and requires no changes to routing logic or config.

See implementation plan in `notes/fix-normalize-tool-ids-plan.md`.
