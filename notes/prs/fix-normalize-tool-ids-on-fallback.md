# PR draft: `fix/normalize-tool-ids-on-fallback`

## Title

Repair fallback tool-call history before Anthropic requests fail (AI-authored)

## Description

### Status

This branch is not ready for a PR yet.

### What users would notice

After implementation, conversations that temporarily fall back to an OpenAI-compatible provider would stop getting stuck when later requests return to an Anthropic provider.

### Intended change

This AI-authored PR would normalize non-Anthropic tool IDs in conversation history before forwarding requests to Anthropic-compatible backends, so `tool_use` and `tool_result` pairs remain structurally valid after provider fallback.

### Missing before PR

- implementation in the branch
- regression tests for mixed-provider tool-call history
- final validation notes
- actual commit(s) to review

### Notes for reviewers

- Background and proposed approach already exist in `/home/agent/repos/ai/claude-code-mux/notes/tool-id-cross-provider-bug.md` and `/home/agent/repos/ai/claude-code-mux/notes/fix-normalize-tool-ids-plan.md`.
- Authored by AI with human review once implemented.
