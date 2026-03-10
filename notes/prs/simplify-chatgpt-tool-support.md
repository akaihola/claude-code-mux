# PR draft: `simplify/chatgpt-tool-support`

## Title

Make ChatGPT tool support more reliable while simplifying the OpenAI bridge (AI-authored)

## Description

### What users notice

ChatGPT-backed models behave more predictably in CCM:

- tool calls survive the OpenAI-to-Anthropic translation path more reliably
- ChatGPT streaming events map cleanly into Anthropic SSE events
- image-bearing requests avoid the broken path that used to lose multimodal content
- reasoning-capable models can keep their native reasoning flow instead of being rerouted unnecessarily

### What changed

This AI-authored change simplifies the ChatGPT/OpenAI bridge by treating ChatGPT Responses events and request items as flexible JSON structures instead of maintaining narrow Rust structs for every event shape. That reduces translation-specific code while preserving tool calls and streaming behavior.

It also folds in two related behavior fixes:

- forward Anthropic thinking budgets as OpenAI Responses reasoning effort
- add `think_exempt_models` so native-reasoning models can skip router-level think rerouting

### Testing

- Ran `cargo test --lib`
- Added/kept focused regression tests in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs` and `/home/agent/repos/ai/claude-code-mux/src/router/mod.rs`

### Notes for reviewers

- Main simplification is in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs`.
- This branch is probably the best successor to `fix/chatgpt-oauth-streaming-404`, but it is broader than a pure simplification PR because it also includes routing and reasoning changes.
- If you want the smallest review, consider splitting `think_exempt_models` and reasoning-effort forwarding into a follow-up PR.
- Authored by AI with human review.
