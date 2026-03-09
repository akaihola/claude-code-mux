# Implementation plan: normalize tool IDs for Anthropic providers

**Branch:** `fix/normalize-tool-ids-on-fallback`  
**Worktree:** `/home/agent/repos/ai/claude-code-mux-fix-tool-ids`  
**Background:** `notes/tool-id-cross-provider-bug.md`

---

## Goal

When CCM forwards a request to an Anthropic-compatible provider (i.e.
`base_url.contains("anthropic.com")`), every `tool_use` ID in assistant
messages and every corresponding `tool_result` `tool_use_id` in user messages
must be paired and must match Anthropic's expected format. If any IDs come
from a non-Anthropic provider (detected as not having the `toolu_` prefix),
rewrite them consistently across the entire message list before sending.

---

## Implementation

### Step 1 – Add a normalization function in `anthropic_compatible.rs`

Add a function `normalize_tool_ids_for_anthropic(request: &mut AnthropicRequest)`
below the existing `sanitize_tool_use_ids` function.

**Algorithm:**

1. Build a `HashMap<String, String>` mapping old ID → new `toolu_`-prefixed ID.
   - Walk all messages.
   - For every `ContentBlock::Known(KnownContentBlock::ToolUse { id, .. })` in
     any assistant message: if `id` does not start with `toolu_`, insert
     `id → format!("toolu_{}", &id[..id.len().min(24)])` (truncate to keep IDs
     short; add a random suffix if collision is a concern, but in practice
     tool-call IDs are unique per conversation).
   - Actually: use a counter or a stable hash rather than truncation, to
     guarantee uniqueness. E.g. `format!("toolu_{:04}", counter)` where counter
     increments per unique old ID seen.

2. If the map is empty (all IDs already start with `toolu_`), return early – no
   rewriting needed.

3. Walk all messages again, rewriting in place:
   - `KnownContentBlock::ToolUse { id, .. }` in assistant messages: replace `id`
     with `map[id]`.
   - `KnownContentBlock::ToolResult { tool_use_id, .. }` in user messages:
     replace `tool_use_id` with `map[tool_use_id]` (if present in map; leave
     unmapped IDs alone – they were already Anthropic-native).

**Signature:**

```rust
/// Rewrite non-Anthropic tool IDs to toolu_-prefixed IDs before sending to
/// Anthropic. OpenAI providers emit call_xxx IDs; this rewrites both the
/// tool_use.id in assistant messages and the matching tool_result.tool_use_id
/// in user messages so they are consistent and Anthropic-compatible.
///
/// Returns the number of IDs rewritten (0 means history was already clean).
fn normalize_tool_ids_for_anthropic(request: &mut AnthropicRequest) -> usize {
    // phase 1: collect all non-toolu_ IDs that need rewriting
    // phase 2: rewrite ToolUse and ToolResult blocks
}
```

Log a `tracing::info!` when rewrites happen so they are visible in CCM logs:

```
🔄 Normalized N non-Anthropic tool ID(s) for Anthropic provider
```

---

### Step 2 – Call the normalizer from both send paths

In `AnthropicCompatibleProvider::send_message` and
`AnthropicCompatibleProvider::send_message_stream`, add the call immediately
after `sanitize_tool_use_ids` (the existing call that's already there):

```rust
sanitize_tool_use_ids(&mut request, is_anthropic);
if is_anthropic {
    let n = normalize_tool_ids_for_anthropic(&mut request);
    if n > 0 {
        tracing::info!("🔄 Normalized {} non-Anthropic tool ID(s) for Anthropic provider", n);
    }
    strip_non_anthropic_thinking(&mut request);
}
```

Both the non-streaming (`send_message`) and streaming (`send_message_stream`)
paths already have this same structure, so the change is symmetric.

---

### Step 3 – Tests

Add unit tests in `anthropic_compatible.rs` or a dedicated test module.

**Test cases to cover:**

1. **No-op when all IDs are already `toolu_`-prefixed.**  
   Input: request with assistant message containing `ToolUse { id: "toolu_abc" }`
   and user message containing `ToolResult { tool_use_id: "toolu_abc" }`.  
   Expected: function returns 0, IDs unchanged.

2. **Single `call_xxx` ID rewritten consistently.**  
   Input: assistant turn with `ToolUse { id: "call_4CfDBg..." }`, followed by
   user turn with `ToolResult { tool_use_id: "call_4CfDBg..." }`.  
   Expected: both rewritten to the same new `toolu_XXXX` ID, function returns 1.

3. **Multiple `call_xxx` IDs in a parallel tool-use block.**  
   Two `ToolUse` blocks in one assistant message, two corresponding `ToolResult`
   blocks in the next user message. All four rewritten consistently; pairings
   preserved.

4. **Mixed history: some `toolu_` some `call_`.** Earlier turns have native
   Anthropic IDs; a later turn has `call_xxx` IDs from a fallback provider.
   Only the `call_xxx` IDs are rewritten; the earlier `toolu_xxx` ones are left
   alone.

5. **`tool_use_id` with no matching `tool_use` (defensive).**  
   A `ToolResult` whose ID is not in the rewrite map should be left unchanged
   rather than panicking or producing a new unmapped ID.

6. **Integration test: request round-trips through `send_message` with mock
   provider.** Use the existing mock/test infra in `tests/` to verify the
   normalizer fires when `is_anthropic = true` and not when `is_anthropic =
false`.

---

### Step 4 – Manual smoke test with message tracing

CCM has a message tracer (`state.message_tracer`). Enable `RUST_LOG=debug` and
send a synthetic request containing `call_xxx` tool use / result pairs to the
CCM endpoint targeting a model whose priority-1 provider is `claude-max`. Verify
in the debug log that:

- The `🔄 Normalized N non-Anthropic tool ID(s)` line appears.
- The outgoing request body (logged at DEBUG level in `send_message`) contains
  `toolu_`-prefixed IDs.
- No 400 error is returned.

---

### Step 5 – Edge cases and follow-up issues to keep in mind

**Codex/Responses API path:**  
`openai.rs` has a separate `transform_to_responses_request` that reconstructs
the flat item list from the (already Anthropic-format) `AnthropicRequest`. The
normalization runs before this transform, so it should be transparent. Verify
that `call_id` fields in the Responses API items use the rewritten IDs
(they come from `KnownContentBlock::ToolUse { id, .. }` which is rewritten in
place before `transform_to_responses_request` runs). Since the normalization
only fires for `is_anthropic` targets, the Codex path (OpenAI provider) is
unaffected.

**opencode-zen → claude-max history already in the wild:**  
The three currently-stuck Claude Code sessions have broken history in their
context. Once this fix is deployed and claude-max's OAuth token is refreshed,
those sessions will succeed on the next user message because the normalization
will fix the IDs before sending.

**Context-window compaction:**  
Claude Code's compaction rewrites the message history into a summary. After a
compaction event the tool-call turns are gone, so the broken IDs are gone too.
The normalization is a safety net for the window between the first cross-provider
response and the next compaction.

**Streaming SSE IDs:**  
The streaming path in `anthropic_compatible.rs` calls `try_send_stream_request`
with the (already normalized) `AnthropicRequest`. The SSE bytes that flow back
from Anthropic will contain `toolu_`-prefixed IDs. On the next turn Claude Code
sends those IDs back, and they will be Anthropic-native. No further normalization
needed for that conversation.

---

## Files changed

| File                                    | Change                                                                                                                     |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `src/providers/anthropic_compatible.rs` | Add `normalize_tool_ids_for_anthropic`. Call it in `send_message` and `send_message_stream` after `sanitize_tool_use_ids`. |
| `tests/normalize_tool_ids.rs` (new)     | Unit tests for the normalizer.                                                                                             |

No changes needed to `server/mod.rs`, `providers/openai.rs`, routing logic, or
config.

---

## Commit plan

1. `feat(providers): add normalize_tool_ids_for_anthropic helper`  
   Pure addition of the function + tests, no call sites yet.

2. `fix(providers): call normalize_tool_ids_for_anthropic before Anthropic requests`  
   Wire the normalizer into `send_message` and `send_message_stream`.

3. `chore: add tracing log line for tool-ID normalization`  
   Ensure the info log fires so operators can see when cross-provider history is
   being repaired.
