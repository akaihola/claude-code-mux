# PR draft: `fix/body-limit-413`

## Title

Fix large conversation requests being rejected by the proxy (AI-authored)

## Description

### What users notice

Large chats and long tool-heavy sessions no longer fail early at the proxy with a `413` request-too-large error.

### What changed

This AI-authored change disables Axum's default 2 MB request body limit in the CCM server so the proxy can forward large conversation payloads instead of rejecting them before they reach the upstream model provider.

### Why this is safe

- CCM is a pass-through proxy for AI conversations, where long sessions can legitimately exceed 2 MB.
- Upstream model providers still enforce their own request-size limits.
- The code change is intentionally small and isolated to the server setup.

### Testing

- Ran `cargo test --lib`

### Notes for reviewers

- This PR is intentionally minimal: one behavioral change in `/home/agent/repos/ai/claude-code-mux/src/server/mod.rs`.
- Authored by AI with human review.
