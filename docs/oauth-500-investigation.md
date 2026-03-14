# OAuth 500 Investigation – Claude Max Sonnet/Opus Access

## Date: 2026-03-14

## Summary

CCM's OAuth tokens for Claude Max (`claude-max` provider) produce HTTP 500
"Internal server error" from Anthropic's API for Sonnet 4.6 and Opus 4.6
models.  Haiku 4.5 works fine.  Claude Code on the same machine, same user,
same plan works perfectly with all models.

## Root Cause (confirmed)

The 500 is caused by the **OAuth token itself** – not by request headers, beta
flags, tool ID sanitization, or request body formatting.

**Proof:**  Copying Claude Code's token (`~/.claude/.credentials.json`) into
CCM's token store (`~/.claude-code-mux/oauth_tokens.json`) and restarting CCM
immediately resolves the 500s.  Sonnet and Opus both work.

The token obtained through CCM's OAuth flow lacks whatever server-side
capability flag Anthropic attaches to tokens that grants Sonnet/Opus access on
the Claude Max plan.

## What Does NOT Cause the 500

These were all ruled out by testing:

| Hypothesis | Test | Result |
|---|---|---|
| Beta headers wrong | Fixed all 3 locations | Still 500 |
| Tool ID sanitization breaking requests | Disabled sanitization | Changed to 400 (pattern violation), not 500 |
| Token expired/invalid | Verified `expires_at` in future, `/v1/models` works | Token valid, 500 only on Sonnet/Opus |
| Anthropic outage | `claude-haiku-4-5` works fine with same token | Model-specific |
| HTTP/1.1 vs HTTP/2 | Tested both with `curl --http1.1` / `--http2` | Same 500 |
| Missing User-Agent or x-stainless headers | Added all SDK-like headers | Same 500 |
| Python SDK vs TypeScript SDK vs curl | Tested all three | All 500 for Sonnet/Opus |
| Wrong `anthropic-version` header | Same `2023-06-01` everywhere | Not the cause |

## What IS Different Between Claude Code's OAuth and CCM's

### Token comparison

| Property | Claude Code | CCM |
|---|---|---|
| Token suffix | `...2jzGPwAA` | `...P_kV5gAA` (varies) |
| Works for Haiku? | Yes | Yes |
| Works for Sonnet? | **Yes** | **No (500)** |
| Works for Opus? | **Yes** | **No (500)** |

### OAuth flow differences

| Parameter | Claude Code | CCM (original) | CCM (current) |
|---|---|---|---|
| `client_id` | `9d1c250a-...` | Same | Same |
| `auth_url` | `claude.ai/oauth/authorize` | Same | Same |
| `token_url` | `api.anthropic.com/v1/oauth/token` | `console.anthropic.com/v1/oauth/token` | `api.anthropic.com/v1/oauth/token` |
| `redirect_uri` (authorize) | `http://localhost:PORT/callback` | `console.anthropic.com/oauth/code/callback` | `http://localhost:13456/api/oauth/callback` |
| `redirect_uri` (exchange) | Same as authorize | Same as authorize | Same as authorize |
| Scopes requested | `user:inference user:mcp_servers user:profile user:sessions:claude_code` | `org:create_api_key user:profile user:inference` | `user:inference user:profile user:sessions:claude_code` |
| Token exchange format | JSON, Content-Type: application/json | Same | Same |
| Token exchange fields | `grant_type`, `code`, `redirect_uri`, `client_id`, `code_verifier`, `state` | Same | Same |

### Key differences that were fixed (but didn't resolve 500 alone):

1. **`token_url`**: Changed from `console.anthropic.com` to `api.anthropic.com`
2. **Scopes**: Removed `org:create_api_key`, added `user:sessions:claude_code`
3. **`redirect_uri`**: Changed from `console.anthropic.com/oauth/code/callback` to
   `http://localhost:13456/api/oauth/callback` (still testing)

### Key difference still under investigation:

The `redirect_uri` is the most likely remaining factor.  Claude Code runs a
localhost HTTP server to capture the OAuth callback directly:
```
http://localhost:PORT/callback
```
CCM originally used the Anthropic console's callback URL:
```
https://console.anthropic.com/oauth/code/callback
```

Anthropic's OAuth server may issue different-capability tokens based on the
`redirect_uri`.  A localhost redirect signals "this is a CLI/desktop app" (like
Claude Code), while a console redirect signals "this is a console/web app" (for
API key management).

## Claude Code OAuth Flow (from binary reverse-engineering)

Extracted from `claude` binary v2.1.76 (`strings` + pattern matching):

### Token exchange function (`aNA`)
```javascript
async function aNA(code, state, codeVerifier, port, isManual = false, expiresIn) {
  let body = {
    grant_type: "authorization_code",
    code: code,
    redirect_uri: isManual
      ? config.MANUAL_REDIRECT_URL           // api.anthropic.com/oauth/code/callback
      : `http://localhost:${port}/callback`,  // localhost callback
    client_id: config.CLIENT_ID,
    code_verifier: codeVerifier,
    state: state,
  };
  if (expiresIn !== undefined) body.expires_in = expiresIn;

  let response = await axios.post(config.TOKEN_URL, body, {
    headers: { "Content-Type": "application/json" },
    timeout: 15000,
  });
  // ...
}
```

### Token refresh function (`ZdH`)
```javascript
async function ZdH(refreshToken, { scopes } = {}) {
  let body = {
    grant_type: "refresh_token",
    refresh_token: refreshToken,
    client_id: config.CLIENT_ID,
    scope: (scopes?.length ? scopes : DEFAULT_SCOPES).join(" "),
  };
  let response = await axios.post(config.TOKEN_URL, body, {
    headers: { "Content-Type": "application/json" },
    timeout: 15000,
  });
  // ...
}
```

### Config constants
```javascript
CLIENT_ID: "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
BASE_API_URL: "https://api.anthropic.com"
TOKEN_URL: `${BASE_API_URL}/v1/oauth/token`   // api.anthropic.com/v1/oauth/token
MANUAL_REDIRECT_URL: `${BASE_API_URL}/oauth/code/callback`
CLAUDE_AI_AUTHORIZE_URL: "https://claude.ai/oauth/authorize"
```

### Scopes stored on working token
```
user:inference, user:mcp_servers, user:profile, user:sessions:claude_code
```

### Beta header for OAuth requests
```
anthropic-beta: oauth-2025-04-20
```
Only `oauth-2025-04-20` is sent as the auth header beta.  Additional beta flags
(like `claude-code-20250219`, `interleaved-thinking-2025-05-14`, etc.) are added
per-request by the SDK, not in the auth flow.

## Changes Made During This Session

### 1. Proactive OAuth token refresh (`src/auth/proactive_refresh.rs`) – NEW FILE

Background task checking all OAuth tokens every 60s, refreshing any within 5 min
of expiry.  Prevents `invalid_grant` errors during idle periods.

### 2. Beta header fix (`src/providers/anthropic_compatible.rs`)

All 3 locations updated:
```
Before: oauth-2025-04-20,claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14
After:  oauth-2025-04-20,claude-code-20250219,interleaved-thinking-2025-05-14,token-counting-2024-11-01
```
- Removed `fine-grained-tool-streaming-2025-05-14` (now GA, not in Claude Code)
- Added `token-counting-2024-11-01` (present in Claude Code)

### 3. OAuth config changes (`src/auth/oauth.rs`)

- `token_url`: `console.anthropic.com/v1/oauth/token` → `api.anthropic.com/v1/oauth/token`
- `redirect_uri`: `console.anthropic.com/oauth/code/callback` → `http://localhost:13456/api/oauth/callback`
- Scopes: removed `org:create_api_key`, added `user:sessions:claude_code`

### 4. Server wiring (`src/server/mod.rs`, `src/auth/mod.rs`)

Wired `spawn_proactive_refresh()` into server startup.

## Workaround (proven working)

Copy Claude Code's token into CCM:
```bash
python3 -c "
import json
from datetime import datetime, timezone

cc = json.load(open('~/.claude/.credentials.json'))['claudeAiOauth']
ccm = json.load(open('~/.claude-code-mux/oauth_tokens.json'))
ccm['claude-max']['access_token'] = cc['accessToken']
ccm['claude-max']['refresh_token'] = cc['refreshToken']
ccm['claude-max']['expires_at'] = datetime.fromtimestamp(
    cc['expiresAt']/1000, tz=timezone.utc
).isoformat()
json.dump(ccm, open('~/.claude-code-mux/oauth_tokens.json', 'w'), indent=2)
"
systemctl --user restart ccm
```

This must be repeated after every token refresh (tokens expire ~every 5 hours).
A permanent solution would be to either:
1. Fix CCM's OAuth flow to produce tokens with full capabilities, OR
2. Implement token syncing from Claude Code's credential file

## Still Open

- Does `redirect_uri=http://localhost:13456/api/oauth/callback` fix the 500?
  (Testing in progress – the redirect_uri must be registered with Anthropic's
  OAuth app for the authorize step to succeed)
- If localhost redirect doesn't work, implement automatic token sync from
  `~/.claude/.credentials.json`
- The `user:mcp_servers` scope is present in Claude Code but not yet in CCM –
  unclear if it affects model access
