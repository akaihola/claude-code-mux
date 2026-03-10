# PR draft: `fix/oauth-token-rotation`

## Title

Fix Claude OAuth sessions getting stuck after token refresh races (AI-authored)

## Description

### What users notice

Claude OAuth-backed models stop getting wedged into repeated re-auth failures when multiple requests try to refresh the same expired token at once.

### What changed

This AI-authored change serializes OAuth token refresh per provider, so only one in-flight refresh can rotate the refresh token at a time. Other concurrent requests wait, re-check the stored token, and reuse the fresh access token instead of sending duplicate refresh attempts that trigger `invalid_grant`.

It also improves diagnostics around refresh success and permanent token invalidation so operators can tell whether a token rotated, stayed unchanged, or needs re-authentication.

### Testing

- Ran `cargo test --lib`

### Follow-up recommended before opening PR

- Add a focused regression test for the per-provider refresh lock / double-check flow.
- Consider moving operational tooling and unrelated repo hygiene out of this branch to keep the diff tightly focused on the race fix.

### Notes for reviewers

- Core code paths are in `/home/agent/repos/ai/claude-code-mux/src/auth/token_store.rs`, `/home/agent/repos/ai/claude-code-mux/src/auth/oauth.rs`, and `/home/agent/repos/ai/claude-code-mux/src/providers/anthropic_compatible.rs`.
- Authored by AI with human review.
