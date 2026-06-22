# Wealthfolio CLI and MCP Agent Access Design

## Overview

This document defines how external AI agents and command line workflows interact
with Wealthfolio. Two runtime scenarios are supported:

- **Desktop app**: Wealthfolio runs as the Tauri desktop app.
- **Docker/self-hosted web**: Wealthfolio runs as the Axum server.

Mobile is excluded from local MCP support. Mobile users use the in-app assistant
or connect through a synced desktop/server environment.

The core design decision: MCP access must go through an already-running
Wealthfolio runtime. The standalone CLI never opens or migrates the SQLite
database directly.

The CLI scope is intentionally narrow: `wealthfolio` only provides
`wealthfolio mcp serve` as an MCP stdio bridge. Human-facing commands are
deferred until the agent tool catalog is stable.

## Release Slicing

| Release              | Scope                                                                                                                                                                    |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **v1** (implemented) | Read-only MCP on BOTH runtimes: agent-tools extraction, embedded Tauri MCP server, server `/mcp` with Personal Access Tokens, Agent Access settings UI. Read tools only. |
| **v1.1**             | CLI stdio bridge (`wealthfolio mcp serve`), activity draft/write tools, and classification suggestions behind explicit scopes.                                           |

Rationale: the demoable value — "ask your agent about your portfolio" — is
entirely read-only. Writes drag in the hardest problems (draft/commit semantics,
partial-failure reporting, CSV sanitization). The draft tools already exist for
the in-app assistant, so v1.1 follows quickly now that the transport layer is
proven. The CLI bridge was deferred out of v1 because both runtimes expose
Streamable HTTP directly, which modern MCP clients support; the bridge is a
compatibility add-on for stdio-only clients, not the front door.

The agent-tools extraction (Phase 1–2) is valuable standalone work even
independent of MCP adoption: it gives the in-app assistant scope enforcement and
audit logging it currently lacks.

## Goals

- Let AI agents query portfolio data through MCP.
- In v1.1, let agents prepare and, where explicitly permitted, record investment
  activities.
- Support desktop MCP clients that only know how to launch stdio servers, and
  clients that speak Streamable HTTP directly.
- Reuse existing Wealthfolio business logic, validation, event handling, and
  security boundaries.
- Converge the in-app AI assistant and external agents on a single audited,
  scoped tool catalog.

## Non-Goals

- No direct `--db` local mode. All access flows through the desktop app or
  server runtime so event handling, auth, and audit logging stay consistent.
- No human-facing CLI commands beyond `wealthfolio mcp serve` in v1.
- No mobile-local MCP server.
- No raw SQL tools.
- No tools for secrets, addon installation, backups, updater, device pairing, or
  arbitrary file access.
- No automatic classification writes.
- No separate user-facing MCP binary.

## Current Repository Context

What already exists (verified against the code):

- **`crates/ai` is already runtime-neutral.** It has no Tauri or Axum
  dependencies. Its `AiEnvironment` trait (`crates/ai/src/env/mod.rs`) already
  abstracts ~18 service handles — it is essentially the `AgentEnvironment` this
  design needs.
- **The real coupling is rig-core, not the runtimes.** All 19 existing assistant
  tools implement rig's `Tool` trait directly (`crates/ai/src/tools/`, ~10.4k
  lines). The extraction work is a dependency inversion: define our own
  `AgentTool` trait and adapt it _to_ rig, instead of tools implementing rig
  directly.
- **`AiEnvironment` needs a split, not a copy.** It currently exposes
  `secret_store()` and `chat_repository()`, which must not be reachable from
  agent tools. It also exposes three concrete (non-trait) services —
  `CashActivityService`, `ActivityTaxonomyAssignmentService`,
  `CategorizationRulesService` — which need trait extraction before mock-based
  tool tests are possible.
- **Service composition is ~90% duplicated** between Tauri's `ServiceContext`
  (`apps/tauri/src/context/providers.rs`, ~620 lines) and the server's
  `AppState` (`apps/server/src/main_lib.rs`, ~840 lines) — roughly 1,400
  duplicated lines wiring ~63 services. MCP adds a third consumer of this graph;
  see Phase 6.
- **Server auth is greenfield for PATs.** Today it is a single Argon2 password
  hash → JWT with cookie/bearer extraction (`apps/server/src/auth.rs`). There is
  no token table, no scopes, nothing to extend.
- **Addon permissions are declaration-only.** The 16 addon permission categories
  (`packages/addon-sdk/src/permissions.ts`) have no runtime enforcement, no
  read/write granularity, and no Rust enum. The agent scope system reuses their
  _names_ for coherence but shares no implementation.
- The Tauri app has no embedded HTTP server today. There is no MCP code anywhere
  in the repo.

Existing Tauri commands and web REST routes remain unchanged. `agent-tools` is a
new parallel agent surface, not a replacement.

## Repository Structure

CLI and MCP source live in the main monorepo. They move in lockstep with
`crates/core`, `crates/storage-sqlite`, migrations, and domain services. A
separate source repository would create schema and behavior drift.

After v1, a thin distribution-only npm wrapper package is acceptable because it
would only download and run the native binary.

**Binary name collision (resolved):** `@wealthfolio/addon-dev-tools` now ships
its CLI as `wealthfolio-addon`, keeping `wealthfolio` as a deprecated alias to
be removed before the native `wealthfolio` CLI ships in v1.1.

## Proposed Architecture

### Workspace Layout

```text
crates/
  agent-tools          # package: wealthfolio-agent-tools
  wealthfolio-mcp      # package: wealthfolio-mcp

apps/
  cli                  # package: wealthfolio-cli, binary: wealthfolio
```

Future, optional: `crates/runtime` (shared service composition),
`packages/wealthfolio-mcp-npm` (distribution wrapper).

### Runtime Hosts

```text
Desktop:
MCP client (HTTP-capable) -> 127.0.0.1:<port>/mcp -> Tauri ServiceContext
MCP client (stdio-only)   -> wealthfolio mcp serve -> 127.0.0.1:<port>/mcp -> Tauri ServiceContext

Docker/self-hosted:
MCP client -> HTTPS /mcp (direct, or bridged via wealthfolio mcp serve --server <url>) -> Axum AppState
```

The CLI is not a third runtime. It is a compatibility bridge for MCP clients
that require stdio.

### `crates/agent-tools`

Owns the runtime-neutral tool catalog.

Responsibilities:

- Define tool names, descriptions, input schemas, output types, scope
  requirements, and access levels (read / draft / write / suggest).
- Execute tools against an abstract `AgentEnvironment`.
- Sanitize tool arguments for audit logging.
- Provide adapters used by `wealthfolio-ai` (rig) and `wealthfolio-mcp`.

It must not depend on Tauri or Axum, own MCP transport, own LLM provider
orchestration, or own app authentication.

**`AgentEnvironment` is the existing `AiEnvironment`, split.** Move the data
service accessors into `agent-tools`; keep assistant-only accessors
(`secret_store`, `chat_repository`) on an extension trait in `crates/ai`:

```rust
// crates/agent-tools — data services only, exact set per current tool usage:
// account, activity, holdings, valuation, allocation, performance, income,
// goal, health, taxonomy, asset, quote, settings, cash-activity,
// activity-taxonomy-assignment, categorization-rules services.
pub trait AgentEnvironment: Send + Sync {
    fn base_currency(&self) -> String;
    fn account_service(&self) -> Arc<dyn AccountServiceTrait>;
    /* ... remaining service accessors ... */
}

// crates/ai — assistant-only additions:
pub trait AssistantEnvironment: AgentEnvironment {
    fn secret_store(&self) -> Arc<dyn SecretStore>;
    fn chat_repository(&self) -> Arc<dyn ChatRepositoryTrait>;
}
```

Service trait names follow the canonical names in `crates/core`. The three
concrete services listed above get traits extracted as part of Phase 1.

```rust
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> serde_json::Value;
    fn required_scopes(&self) -> &'static [AgentScope];
    fn access_level(&self) -> AgentToolAccess;
    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError>;
}
```

A rig adapter in `crates/ai` wraps `AgentTool` into rig's `Tool` so the in-app
assistant keeps identical behavior. This prevents the assistant and MCP from
drifting.

**Tool names are stable identifiers.** `ChatThreadConfig` snapshots tool
allowlists by name and `normalize_tools_allowlist` already handles legacy name
expansion. Tools keep their existing names during extraction; any future rename
is a migration (allowlist expansion entry), not a simple edit.

### `crates/wealthfolio-mcp`

Owns MCP protocol integration.

Responsibilities:

- Convert `agent-tools` into MCP tools/resources/prompts.
- Enforce scopes at the MCP boundary.
- Call audit logging hooks.
- Expose a server builder runnable over local HTTP inside Tauri, HTTP inside
  Axum, and the v1.1 stdio bridge.

It must not build repositories/services, open SQLite, or know Tauri/Axum
internals beyond adapter hooks.

**SDK:** default to `rmcp` (the official Rust MCP SDK) for transport, session
handling, and protocol negotiation. Hand-rolling the protocol is only justified
if `rmcp` cannot support the embedded-HTTP-in-Tauri shape; validate this first
in Phase 3, as it is the largest unknown in the plan.

**Protocol version:** do not pin. Implement spec-standard version negotiation
and declare a _minimum_ supported version (Streamable HTTP transport; the
deprecated HTTP+SSE transport is not supported).

### `apps/cli`

One binary: `wealthfolio`. v1 surface:

```text
wealthfolio mcp serve
  Bridge stdio to the running desktop app MCP server discovered locally.

wealthfolio mcp serve --server https://wealthfolio.example.com   (v1.1)
  Bridge stdio to a remote server MCP endpoint.
```

If the desktop app is not running, local bridge mode fails with:

```text
Open Wealthfolio, enable MCP Server in Settings, then try again.
```

## Desktop Embedded MCP

The Tauri app hosts a local MCP HTTP server when enabled in settings.

Defaults:

- disabled by default
- bind to `127.0.0.1`
- **stable default port 8639, with fallback**: try 8639 first (overridable via
  the `mcp_server_port` setting) so Streamable-HTTP-capable clients can connect
  directly without the CLI; fall back to a random high port on conflict. The
  lock file is always the source of truth; the CLI bridge is the escape hatch
  for stdio-only clients, not the front door.
- read tools enabled; draft/write tools do not exist until v1.1 and ship
  disabled by default
- local token required

The embedded server uses the existing `ServiceContext`. This ensures no second
process opens SQLite, writes go through existing services, domain events flow
through the app's event sink, and UI updates stay consistent.

### Local HTTP Security

The local MCP HTTP server must:

- bind to loopback only
- reject requests without a valid bearer token
- validate the `Origin` header before handling MCP requests: v1 allows only no
  `Origin` or `Origin: null` (a configurable allowlist of known local clients
  can be added later if a browser-based local client needs it)
- never log the local token
- be unstartable on mobile: the `mcp_*` Tauri commands and the server
  orchestration are `#[cfg(desktop)]`-gated and return an error on iOS/Android —
  the UI hiding is not the enforcement boundary

Origin validation is required even on `127.0.0.1` to reduce DNS rebinding and
malicious-browser request risk.

### Discovery File

When the embedded server starts, Tauri writes a discovery file in the app data
directory:

```text
<app_data>/mcp.lock
```

Both Tauri and the CLI must resolve the same app data path for the desktop
identifier `com.teymz.wealthfolio` — via a shared helper or an explicitly tested
`directories::ProjectDirs` mapping. If the app identifier changes, the lock-file
and keyring conventions migrate together.

```json
{
  "lockFileVersion": 1,
  "port": 8639,
  "pid": 12345,
  "startedAt": "2026-05-17T00:00:00Z",
  "tokenFingerprint": "sha256:..."
}
```

Rules:

- The lock file never contains the token; it is only a discovery hint.
- The CLI validates PID, port, health endpoint, and token fingerprint before
  bridging.
- Tauri deletes the file on clean shutdown.
- The CLI tolerates stale lock files by failing health validation.

### Local Token

Single local token in v1:

- Generated by Tauri; 256 bits of entropy, displayed as `wfl_<base64url>`.
- Stored in the OS keyring via the app's existing `SecretStore` under secret key
  `mcp.local` (keyring service id `wealthfolio_mcp.local`, from the shared
  `format_service_id` helper in `crates/core`).
- Never written to `mcp.lock`; never logged.
- Rotatable from settings; rotation closes active sessions and invalidates
  copied MCP client configs (confirmation prompt explains this).
- Per-client enrollment is deferred until write scopes need granular revocation.

### Settings UI

Settings section: **Agent Access** (final naming open).

Controls:

- Enable local MCP server; auto-start with Wealthfolio.
- Show bind address and port.
- Rotate local token (with disconnect confirmation).
- Copy MCP client config — the direct Streamable HTTP config in v1; a stdio
  config that launches `wealthfolio mcp serve` ships with the CLI in v1.1.
- Access preset: read-only (v1). v1.1 adds read + activity writes and read +
  writes + classification suggestions.
- Show recent agent activity (audit log view).
- Show/disconnect active sessions where the MCP adapter tracks them. If session
  enumeration isn't available from the SDK in v1, keep token rotation and audit
  logging and defer the live session list.

## Docker/Web MCP

The Axum server exposes `/mcp` in the same runtime that serves the web app,
using the existing `AppState`. It is enabled with `WF_MCP_ENABLED=true` (default
false) and mounted top-level — outside the JWT-protected `/api/v1` subtree and
outside its 300s request timeout, which would kill SSE streams.

Authentication: Personal Access Tokens, sent as bearer tokens, with explicit
scopes. Server MCP always requires PATs — there is no trusted reverse proxy
bypass. Note this is net-new infrastructure: the server has no token table or
scope concept today.

Host validation: `WF_MCP_ALLOWED_HOSTS` (comma-separated) enables a strict
`Host`-header allowlist on `/mcp`. When unset, Host validation is disabled —
rmcp's loopback-only default would break real deployments behind a domain, and
the PAT bearer requirement is the security boundary (browsers cannot attach
`Authorization` headers cross-site, so DNS rebinding gains nothing). Set it when
deploying behind a known hostname for defense in depth.

### Personal Access Tokens

```text
personal_access_tokens
- id
- name
- token_prefix
- token_hash
- scopes_json          -- JSON array of scope strings
- expires_at           -- optional; UI recommends but does not force expiry
- last_used_at
- revoked_at
- created_at
```

Rules:

- Store only token hashes. High-entropy random tokens hashed with SHA-256
  (Argon2 is unnecessary for high-entropy secrets).
- Show the full token only once at creation; use the prefix for lookup and
  display.
- Log token fingerprint, never the token, in audit records.

PAT management lives in the web settings UI.

## Tool Catalog

Tools keep their existing `crates/ai` names — no renames during extraction (see
"Tool names are stable identifiers" above).

### v1 Read Tools (all exist today in `crates/ai`)

```text
get_accounts
get_cash_balances
get_holdings
get_asset_allocation
get_performance
get_valuation_history
search_activities
get_income
get_goals
get_health_status
list_asset_taxonomies
get_asset_taxonomy_assignments
list_categorization_context
```

### v1.1 Draft/Write/Suggest Tools (exist today, exposed to MCP in v1.1)

```text
record_activity                    -- draft + explicit commit
record_activities
import_csv                         -- preview/mapping only
propose_transaction_categories
create_categorization_rule
prepare_asset_classification
```

Rules:

- Draft tools are safe by default; committing requires `activities:write`.
- CSV content must not be persisted in raw audit logs (the assistant already
  redacts `import_csv` arguments; agent-tools generalizes this as per-tool audit
  sanitization).
- Activity writes use existing activity services and validation.
- Suggestions return proposed assignments and rationale; agents never directly
  alter allocation semantics.

### Deferred Indefinitely

Classification writes, taxonomy create/edit/delete, activity/account deletion,
backups, secrets, addon install, device pairing.

## Scope Model

Scopes reuse the addon permission category _names_ (`accounts`, `portfolio`,
`activities`, `performance`, `financial-planning`, ...) with an action suffix.
This is naming alignment only: the addon permission system is declaration-only
with no runtime enforcement, so nothing is shared beyond vocabulary. The agent
scope enum is a small hand-written Rust enum and is the first enforced
permission model in the codebase.

**Define only scopes that gate shipped tools.** Speculative scopes are liability
— they appear in token UIs and imply capabilities that don't exist.

v1 scopes:

```text
accounts:read            get_accounts, get_cash_balances
portfolio:read           get_holdings, get_asset_allocation,
                         get_valuation_history, get_income
performance:read         get_performance
activities:read          search_activities
financial-planning:read  get_goals
health:read              get_health_status
classification:read      list_asset_taxonomies, get_asset_taxonomy_assignments,
                         list_categorization_context
```

v1.1 adds:

```text
activities:write         record_activity / record_activities commit, import_csv
classification:suggest   propose_transaction_categories,
                         create_categorization_rule,
                         prepare_asset_classification
```

Presets: `read-only` (v1); `read-activity-write` and
`read-activity-write-classification-suggest` (v1.1).

Scope enforcement happens at the `agent-tools` boundary, before tool execution.
Runtime hosts may also enforce transport-level auth, but tool execution never
relies on transport auth alone.

## Audit Logging

MCP and agent-tool execution write audit rows to SQLite in both desktop and
server mode.

```text
mcp_audit_log
- id
- session_id          -- groups calls from one agent session
- actor_kind          -- local_token | pat | desktop_bridge
- actor_fingerprint
- tool
- scopes_json
- args_summary        -- sanitized per-tool
- outcome             -- success | denied | error
- error_message
- created_at
```

Rules:

- Never log secrets, full CSV content, or raw tokens.
- Keep rows forever in v1; manual purge button in settings.
- Index on `(created_at, tool)` from day one for the settings activity view.

## Threat Model Notes

- With auto-start enabled, the local MCP server may run whenever the app runs.
  The local token is the boundary between local processes and agent access: high
  entropy, never logged.
- The local token protects against accidental local access, not a fully
  compromised OS user account.
- Local HTTP requests validate `Origin` even on loopback.
- Server MCP requires PATs even behind an authenticating reverse proxy.
- Agent tools never expose raw database access, secrets, backups, addon
  installation, or device pairing.

## Data Flow

### Desktop

```text
MCP client (stdio) -> wealthfolio mcp serve
  -> CLI reads <app_data>/mcp.lock + token from keyring
  -> CLI validates health endpoint, bridges stdio to local HTTP MCP
MCP client (HTTP) -> 127.0.0.1:<port>/mcp directly
  -> Tauri MCP adapter checks token + Origin + scopes
  -> agent-tools execute via ServiceContext services
  -> domain events update UI and derived data as usual
```

### Docker/Web

```text
MCP client -> HTTPS /mcp (direct or CLI-bridged)
  -> bearer PAT authenticates; scopes checked at agent-tools boundary
  -> AppState services execute; server events flow normally
```

## Failure Modes

Desktop: missing lock file → "open Wealthfolio and enable MCP"; stale lock file
→ health check fails with remediation; token mismatch after rotation → re-copy
config; port conflict → fall back to random port, rewrite lock file; session
disconnect → close connection and audit.

Server: missing/revoked/expired PAT → unauthorized; insufficient scope → denied
and audited; MCP disabled → clear setup error.

Tool execution: validation failures return structured tool errors; denied calls
are audited; partial write failures return enough information for the agent to
explain what was and was not saved.

## Implementation Plan

### Phase 1: Invert the rig dependency (the real extraction)

- Create `crates/agent-tools` with the `AgentTool` trait and `AgentScope` enum.
- Split `AiEnvironment`: data accessors move to `AgentEnvironment` in
  agent-tools; `secret_store`/`chat_repository` stay on an
  `AssistantEnvironment` extension trait in `crates/ai`.
- Extract traits for the three concrete services (`CashActivityService`,
  `ActivityTaxonomyAssignmentService`, `CategorizationRulesService`).
- Write the rig adapter in `crates/ai` (wraps `AgentTool` as rig `Tool`).
- Migrate the first two tools (`get_accounts`, `get_holdings`) with parity tests
  proving schemas and outputs match the current assistant tools.

### Phase 2: Migrate the catalog

- Migrate remaining read tools, then draft/suggest tools, keeping existing
  names.
- Add scope metadata, access levels, and per-tool audit sanitization
  (generalizing the existing `import_csv` redaction).
- Verify the in-app assistant behavior is unchanged (allowlist tests,
  `ChatThreadConfig` compatibility).

### Phase 3: Desktop embedded MCP

- Validate `rmcp` fits the embedded-HTTP-in-Tauri shape (largest unknown — do
  this first).
- Create `crates/wealthfolio-mcp`; embed the local HTTP MCP server in Tauri.
- Local token in keyring, `mcp.lock` discovery file, default-port-with- fallback
  binding, Origin validation.
- `mcp_audit_log` migration and settings UI (enable, rotate, copy config,
  activity view).

### Phase 4: CLI stdio bridge

- Resolve the npm `wealthfolio` bin collision first.
- Create `apps/cli`; implement `wealthfolio mcp serve` bridging stdio to the
  desktop server.
- Ship installable client config examples (stdio and direct HTTP).

**v1 ships here, read-only. v1.1 enables the draft/write tools already migrated
in Phase 2.**

### Phase 5: Docker/web MCP

- Server `/mcp` endpoint on `AppState`.
- PAT table, creation/revocation, scope enforcement; web settings UI for PAT
  management and audit log.
- `wealthfolio mcp serve --server <url>` bridge mode.

### Phase 6: Runtime extraction

Extract shared service composition into `crates/runtime` once agent-tools and
MCP prove the interfaces. The duplication is already real (~1,400 lines, ~90%
overlapping wiring of ~63 services across `ServiceContext` and `AppState`), and
MCP makes it a three-consumer problem. Trigger: do this when the next service
addition requires touching both compositions plus the `AgentEnvironment` trait.
Keep shell-specific code (events, secrets, auth) in Tauri and Axum.

## Testing Strategy

Agent tools: unit-test with mock services; snapshot JSON schemas; parity tests
against current assistant tools; scope denials happen before execution.

Desktop MCP: starts only when enabled; loopback-only; lock file has no token;
rejects missing/invalid token and disallowed Origins; CLI handles stale lock
files; (v1.1) activity commit updates derived data through existing domain
events.

Server MCP: requires PAT; enforces scopes; rejects expired/revoked tokens; works
behind a reverse proxy; audits success/denied/error.

Audit: sanitizes secrets and CSV content; records session, actor fingerprint,
tool, scopes, outcome, timestamp; manual purge removes rows.

Regression: existing Tauri commands, web REST routes, and in-app assistant tool
calls unchanged; no direct SQLite access introduced in CLI/MCP.

## Resolved Decisions (v1 implementation)

- Settings section is named **Agent Access** (under Connections).
- Desktop default port: **8639**, random fallback on conflict; `mcp_server_port`
  setting overrides. Settings keys: `mcp_server_enabled`,
  `mcp_server_auto_start`.
- SDK: **rmcp 1.7** (manual `ServerHandler`, runtime tool registration,
  `StreamableHttpService` mounted in axum on both hosts; host auth context flows
  via forwarded `http::request::Parts` extensions — covered by a regression
  test).
- npm bin collision: dev-tools CLI renamed to `wealthfolio-addon` with a
  deprecated `wealthfolio` alias.
- PAT creation UX: name + fixed expiry options (30/90 days, 1 year, none);
  scopes fixed to the read-only preset; token shown once.
- Server MCP fail-closed: `WF_MCP_ENABLED=true` on a non-loopback address
  requires auth configured, with no `WF_AUTH_REQUIRED=false` escape hatch
  (otherwise anyone could mint PATs).

## Open Decisions

- v1.1 CLI distribution (native binary vs npm wrapper) and whether it absorbs
  the addon dev tools.
- Per-client local desktop tokens before write scopes ship.

## Default Decisions

- One user-facing binary: `wealthfolio`; CLI is a stdio bridge only.
- No local direct DB mode, ever, for agents.
- Desktop MCP embedded in Tauri; Docker MCP embedded in Axum.
- `AgentEnvironment` is the split of the existing `AiEnvironment`, not a new
  parallel trait.
- Tool names are stable; existing `crates/ai` names are kept.
- Scopes are defined only for shipped tools; addon category reuse is
  naming-only.
- Single local desktop token in v1; lock file never contains secrets.
- Audit log stored in SQLite with `session_id`.
- v1 is read-only; activity writes land in v1.1 behind explicit scope;
  classification stays read/suggest-only.
