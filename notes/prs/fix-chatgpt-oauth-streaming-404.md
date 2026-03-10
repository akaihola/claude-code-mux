# PR draft: `fix/chatgpt-oauth-streaming-404`

## Title

Fix ChatGPT OAuth sessions failing to stream or call tools reliably (AI-authored)

## Description

### What users notice

ChatGPT OAuth-backed models stop failing in common real-world cases:

- streaming requests no longer hit the wrong endpoint and fail with `404`
- tool-calling responses are translated correctly
- image-bearing requests avoid the lossy Responses API path that dropped content
- non-streaming tool responses now report `tool_use` instead of incorrectly ending the turn

### What changed

This AI-authored change teaches the OpenAI provider to distinguish between standard OpenAI streaming and ChatGPT `/codex/responses` streaming, then translate ChatGPT's SSE event format into Anthropic-style SSE events that Claude Code expects.

It also improves the Responses API request/response bridge so tool calls are preserved in both streaming and non-streaming paths, while keeping multimodal requests on the existing chat-completions path until full image support exists in the serializer.

### Testing

- Ran `cargo test --lib`
- Added focused regression tests in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs` for:
  - ChatGPT tool-call streaming to Anthropic `tool_use`
  - non-streaming function-call parsing
  - image-bearing requests staying off the Responses API path

### Notes for reviewers

- The essential behavior change is in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs`.
- Before opening this PR, consider trimming unrelated repo-maintenance files from the branch for a smaller diff.
- Authored by AI with human review.
