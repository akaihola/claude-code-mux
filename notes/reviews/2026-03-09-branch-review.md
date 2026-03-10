# Branch review – 2026-03-09

Repository: `/home/agent/repos/ai/claude-code-mux`
Reviewer: AI assistant
Scope: `fix/body-limit-413`, `fix/chatgpt-oauth-streaming-404`, `fix/normalize-tool-ids-on-fallback`, `fix/oauth-token-rotation`, `simplify/chatgpt-tool-support`

## Findings

### 1. `fix/normalize-tool-ids-on-fallback` is not review-ready

- The branch name and notes in `notes/tool-id-cross-provider-bug.md` and `notes/fix-normalize-tool-ids-plan.md` describe a planned fix, but `git log main..fix/normalize-tool-ids-on-fallback` shows no commits.
- There is therefore no implementation diff, no tests, no docs update, and no commit message to review.
- Recommendation: either drop this branch from the active PR set until code exists, or implement the planned normalizer plus tests before opening a PR.

### 2. `fix/chatgpt-oauth-streaming-404` includes unrelated branch-local repo maintenance

- Diff vs `main` includes `.cargo/config.toml`, `AGENTS.md`, and `TASKS.md`, in addition to the actual OpenAI provider fix.
- The user-facing fix lives in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs`, but the extra files broaden the PR without being necessary to ship the behavior change.
- Recommendation: split the provider fix from repo-maintenance/docs changes, or at minimum drop `.cargo/config.toml`, `/home/agent/repos/ai/claude-code-mux/AGENTS.md`, and `/home/agent/repos/ai/claude-code-mux/TASKS.md` from the PR branch to minimize review noise.

### 3. `fix/chatgpt-oauth-streaming-404` and `fix/oauth-token-rotation` do not demonstrate red/green TDD for the new bug class

- Both branches pass `cargo test --lib`, but `fix/oauth-token-rotation` adds no regression test for the concurrent-refresh race, despite the change centering on new locking behavior in `/home/agent/repos/ai/claude-code-mux/src/auth/token_store.rs` and `/home/agent/repos/ai/claude-code-mux/src/providers/anthropic_compatible.rs`.
- `fix/chatgpt-oauth-streaming-404` does add OpenAI unit tests, which is good, but there is no evidence in commit structure or notes that the failure was reproduced first and then fixed.
- Recommendation: add focused regression tests for token-refresh serialization and, if you want to claim TDD, make commit messages or PR body explicit about the failing test added first.

### 4. `fix/oauth-token-rotation` can likely be reduced before review

- The essential fix appears to be the per-provider async refresh mutex in `/home/agent/repos/ai/claude-code-mux/src/auth/token_store.rs` and the double-checked refresh path in `/home/agent/repos/ai/claude-code-mux/src/providers/anthropic_compatible.rs`.
- The branch also adds `.gitignore` entry `notes/`, extensive operational logging, and a new watcher script at `/home/agent/repos/ai/claude-code-mux/scripts/ccm_429_watcher.py` that looks like incident tooling rather than part of the minimal product fix.
- Recommendation: keep the locking fix; move the watcher script and `notes/` ignore rule to separate ops/docs branches unless they are intentionally part of the shipped change.

### 5. `simplify/chatgpt-tool-support` is directionally good but mixes simplification with new behavior

- Compared with `fix/chatgpt-oauth-streaming-404`, this branch does simplify `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs` substantially by using `serde_json::Value` for ChatGPT responses events and flattened Responses API items.
- However it also adds new behavior outside that scope: reasoning-effort forwarding in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs`, `think_exempt_models` in `/home/agent/repos/ai/claude-code-mux/src/cli/mod.rs`, and related router changes in `/home/agent/repos/ai/claude-code-mux/src/router/mod.rs`.
- Recommendation: if the goal is a clean simplification PR, peel off reasoning/routing changes into a separate branch; otherwise rename the branch/PR to reflect the broader scope.

## Branch-by-branch assessment

### `fix/body-limit-413`

- Diff is minimal and appropriate: disabling Axum's default body limit in `/home/agent/repos/ai/claude-code-mux/src/server/mod.rs`.
- Style is consistent and the inline comment is concise.
- Tests: only existing library suite was run; there is no dedicated regression coverage for oversized request bodies.
- Docs: probably no README update needed unless request-size limits are documented somewhere.
- Commit message `fix: remove axum's 2MB default body limit` is clear.
- Verdict: good candidate for a very small PR as-is, with optional integration coverage later.

### `fix/chatgpt-oauth-streaming-404`

- Core fix in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs` appears sound: correct OAuth responses endpoint, add ChatGPT-specific SSE handling, preserve image-bearing requests by staying off the lossy Responses serializer, and map tool stop reasons correctly.
- Tests in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs` match existing project style: local unit tests close to the implementation.
- The branch is larger than necessary because of unrelated `.cargo/config.toml`, `/home/agent/repos/ai/claude-code-mux/AGENTS.md`, and `/home/agent/repos/ai/claude-code-mux/TASKS.md` changes.
- Commit messages are understandable, though they could be squashed into a tighter narrative before opening a PR.
- Verdict: should be trimmed, then it is a strong PR candidate.

### `fix/normalize-tool-ids-on-fallback`

- No implementation commits beyond `main`.
- Notes are detailed and useful, but belong on `notes` only until code exists.
- Verdict: not ready for PR.

### `fix/oauth-token-rotation`

- The concurrency fix is plausible and aligned with the reported `invalid_grant` race.
- Style is mostly consistent, though comments/logging are more verbose than the rest of the crate in places.
- Missing regression tests for the new concurrency guard are the main gap.
- The new watcher script and `notes/` ignore rule inflate the diff and should likely be split out.
- Commit message `fix: prevent OAuth token-rotation race (thundering-herd invalid_grant)` is clear.
- Verdict: simplify before review and add at least one focused regression test.

### `simplify/chatgpt-tool-support`

- Best implementation shape of the ChatGPT/OpenAI work reviewed here.
- Test style matches upstream practice: `#[test]`s colocated in `/home/agent/repos/ai/claude-code-mux/src/providers/openai.rs` and `/home/agent/repos/ai/claude-code-mux/src/router/mod.rs`.
- Still not minimal: includes reasoning/routing features that are not strictly part of ChatGPT tool support simplification.
- Commit history is readable, but if this becomes the preferred PR branch it should probably supersede `fix/chatgpt-oauth-streaming-404` rather than coexist with it.
- Verdict: likely the best base for a merged PR, after trimming nonessential routing/reasoning changes if you want the smallest possible diff.

## Validation performed

- Ran `cargo test --lib` on `fix/body-limit-413` – pass.
- Ran `cargo test --lib` on `fix/oauth-token-rotation` – pass.
- Ran `cargo test --lib` on `fix/chatgpt-oauth-streaming-404` – pass.
- Ran `cargo test --lib` on `simplify/chatgpt-tool-support` – pass.
- Did not run long `cargo build --release`; project instructions say to use `interactive_shell` dispatch for that path.

## Suggested simplification plan

1. Keep `fix/body-limit-413` as a standalone tiny PR.
2. Treat `simplify/chatgpt-tool-support` as the successor to `fix/chatgpt-oauth-streaming-404`.
3. From `simplify/chatgpt-tool-support`, split out reasoning-effort forwarding and `think_exempt_models` if the goal is minimal diff.
4. Reduce `fix/oauth-token-rotation` to the locking fix plus tests; move watcher/ops artifacts elsewhere.
5. Leave `fix/normalize-tool-ids-on-fallback` on hold until implementation exists.
