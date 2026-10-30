# M1 Desktop App — Design

Date: 2026-10-30. Status: draft for review. Builds on M0 (`av-core` + `av` CLI, branch `m0-data-spike`; findings in `docs/m0-findings.md`).

## 1. Goal

A macOS desktop app that shows every local Claude Code session at a glance — what each one is doing, which need you, and what their subagents are up to — and tells you when one needs you or finishes. Personal tool, view-only.

**Scope (PRD F1–F8 + F14):** live discovery, agent cards, subagent tree, live transcript detail, filters and grouping, desktop notifications, menu bar "needs you" badge, and a hook installer (pulled forward from M2).

**Constraints (fixed decisions):** Tauri 2 (Rust + React + TypeScript + Tailwind); macOS only; local sessions only; view-only (never sends signals or input to sessions); personal use, unsigned or ad-hoc-signed build.

**Success targets:**
- A new session appears in ≤ 2 s; a status change shows in ≤ 1 s.
- With 20 agents: idle CPU < 2%, memory < 300 MB.
- Incremental update for one appended transcript line < 5 ms.
- Manual exit check (section 9) passes.

**Out of scope:** cost estimates (M2), history/search across past sessions, any action on sessions (v2), other OSes, remote sessions, code signing and notarization.

## 2. Decisions made during brainstorming

1. **Hooks are in M1.** The app can install `Notification` and `Stop` hooks into `~/.claude/settings.json` (backup first, uninstall supported). Without hooks the app falls back to the `waiting?` heuristic and never notifies on it.
2. **Finished notification** fires on `working` → `idle`/`waitingInput` after at least N seconds of work (N = 30, adjustable), and when a background subagent finishes. Permission, waiting-for-input and error notifications always fire (each can be turned off).
3. **App shape:** normal app (Dock icon, main window) plus a menu bar icon with the needs-you count and an agent dropdown. Closing the window hides it; the app keeps running until Quit.
4. **Token number on a card = context size now:** the last real API call's `input + cache_creation + cache_read` tokens against the model's context limit (e.g. "182k / 200k"). Totals (input, output, cache write, cache read) appear only in the detail view.
5. **Architecture:** the Rust backend owns live state, reads files incrementally, and pushes changes to the UI (approach A). Rejected: UI polling full scans (misses CPU target, notifications die with the webview) and a separate daemon (too heavy for a personal tool).

## 3. Architecture

### Repo layout

```
crates/av-core/          shared library (existing, extended)
crates/av-cli/           `av` CLI (existing, plus `av hook`)
apps/desktop/
  src-tauri/             Rust shell: commands, events, tray, notifier (workspace member)
  src/                   React + TypeScript + Tailwind UI
```

### `av-core` modules

Each module has one job and is testable without threads or a real `~/.claude`.

| Module | Job |
|---|---|
| `model` (new) | The frozen IPC types: `Agent`, `Subagent`, `Status`, `Context`, `Tokens`, `TranscriptEntry`, `Change`. All serialize as camelCase; enums are internally tagged with `kind`. Internal types are not exposed. |
| `tail` (new) | Incremental reading of one transcript: keeps byte offset, a carried partial last line, and the running summary. Re-reads from 0 when the file shrinks or its inode changes. The M0 `summarize_transcript` logic moves here so full and incremental reads share one code path. |
| `engine` (new) | The state store. Inputs: `RegistryChanged`, `TranscriptGrew(path)`, `HookEvent`, `Tick(now)`. Output: `Vec<Change>` (`AgentAdded`, `AgentUpdated`, `AgentRemoved`). Pure: no threads, no file watching, `now` passed in. |
| `watch` (new) | Watches `~/.claude/sessions/`, `~/.claude/projects/` and `~/.claude/agent-viewer/` with the `notify` crate (FSEvents), debounced 100 ms, and maps paths to engine inputs. Falls back to a 2 s full rescan if the watcher fails. |
| `hooks` (new) | Install / uninstall / status of the app's hooks in `~/.claude/settings.json`; parsing of `events.jsonl` lines. |
| `status` (existing, changed) | Priority order in section 4. Hook status wins over the heuristic; an open `Agent` tool call no longer counts as pending. |
| `registry`, `liveness`, `paths`, `subagents` | Existing; reused as is. |

`scan` stays for the CLI. `av list`/`av watch` keep working unchanged.

### Tauri shell (`src-tauri`)

- A background thread runs `watch` → `engine`, plus a 1 s `Tick` (liveness, `waiting?` timing, durations). Each non-empty change list is emitted as the Tauri event `agents://changed`.
- Commands: `list_agents()`, `get_transcript(agentId, subagentId?, before?)`, `install_hooks()`, `uninstall_hooks()`, `hooks_status()`, `get_settings()`, `set_settings(settings)`.
- **Notifier:** consumes engine changes, applies the rules in section 6, sends macOS notifications.
- **Tray:** needs-you count and agent menu (section 5).
- Closing the main window hides it. Quit is in the tray menu and the app menu.

### React UI

A Zustand store holds agents. On load it calls `list_agents()`, then applies `agents://changed` events. Everything else in the UI is view state (filters, grouping, selection).

### `av` CLI

Unchanged for users, plus `av hook` (section 6). The app bundles `av` at `<App>.app/Contents/MacOS/av` and the installed hook points there.

## 4. Data model and status

### Agent

```
id              sessionId
pid
cwd
projectName     last path segment of cwd
gitBranch
title           agent-name record > registry name > first user prompt
entrypoint      from registry (cli, vscode, sdk-…)
model
status          Status (below)
statusSince     when it entered the current status
lastActivity
context         { used, limit | null }
tokens          { input, output, cacheWrite, cacheRead }  totals, counted once per message.id (last usage wins)
pendingTool     { name, since } | null
subagents       [Subagent]
transcriptPath
```

**Context:** `used` = `input_tokens + cache_creation_input_tokens + cache_read_input_tokens` of the last non-synthetic assistant record. `limit` = 1,000,000 if the model id ends in `[1m]` or any call has used more than 200,000; otherwise 200,000. If the model is unknown, `limit` is `null` and the card shows only `used`.

**Subagent:** `id`, `agentType`, `description`, `status` (`running` | `done` | `stopped`, as in M0), `lastActivity`, `tokens`, `pendingTool`.

### Status

Serialized as `{ "kind": "...", ...fields }`. First match wins:

| # | kind | Rule |
|---|---|---|
| 1 | `exited` | Process gone or pid reused (M0 liveness). |
| 2 | `needsPermission { tool? }` | Active hook Notification of the permission type. |
| 3 | `error { kind }` | Last record is an API error (M0 shapes). |
| 4 | `waitingInput` | Active hook Notification of the waiting-for-input type, or registry `idle` after a turn ended (`turn_duration` record or `Stop` event). |
| 5 | `maybeWaiting` (shown as "waiting?") | Only when hooks are not installed: registry `busy` and a non-`Agent` tool open > 5 s. |
| 6 | `working` | Registry `busy`. |
| 7 | `idle` | Alive, nothing else applies. |
| 8 | `unknown { reason }` | Files unreadable. |

### Hook events

- `Notification` → `needsPermission` or `waitingInput`. The type comes from the payload's notification-type field when present, otherwise from the message text ("needs your permission" → permission; "waiting for your input" → input). An unrecognized message is ignored.
- `Stop` → marks the turn as ended (used by the finished rule).
- **Clearing:** a hook status lasts until any of: new transcript records with a timestamp after the event, a newer hook event for that session, or the process exiting.
- **Replay at startup:** only events from the last 10 minutes for sessions that are still alive. When the app opens, `events.jsonl` is trimmed to its last 1 MB.

### Needs-you count

Agents in `needsPermission`, `waitingInput` or `error`. `waiting?` is never counted.

## 5. UI

### Main window

Toolbar on top, agent list on the left, detail panel on the right.

**Toolbar:**
- Search over title, project and branch.
- Filter chips with counts: Needs you · Working · Idle · Exited.
- Group by: None / Project / Entrypoint.
- "Show exited" toggle (off by default). When on, it shows sessions that exited in the last 24 h.

**Agent card (one row):**
- Status dot and label (`waiting?` is faded).
- Title, `project · branch`, entrypoint.
- Time in status ("working 3m").
- Pending tool ("Bash · 12s").
- Context bar ("182k / 200k"), amber above 80%, red above 95%.
- Count of running subagents.

**Sorting:** Needs you → Working → Idle → Exited, then most recent activity first.

**Expanding a card** shows its subagent tree: type, description, status, time since last activity.

**Detail panel (agent or subagent):**
- Header: cwd, pid, model, started, token totals (input / output / cache write / cache read), "Open folder in Finder".
- Transcript: the last 200 records loaded from the end of the file, "Load earlier" pages back by 200, and new records appear live. Display rules:
  - User prompts and assistant text show as messages.
  - Tool calls show as one line (`Bash: npm test`) and expand to input and result, each cut at 4 KB.
  - Thinking blocks are collapsed.
  - `system`, `attachment` and other non-conversation record types are hidden.
- The view auto-scrolls while you are at the bottom.
- No action buttons.

**Empty state:** "No Claude Code sessions running" plus the watched path.

**Hooks banner** (when hooks are not installed and not dismissed): "Install hooks to get reliable permission alerts" with Install and Dismiss buttons.

The theme follows macOS light/dark. The window remembers its size and position.

### Settings window (⌘,)

- **Hooks:** status, Install / Uninstall / Repair, path of the last backup.
- **Notifications:** on/off for each of needs permission, waiting for input, finished, error, subagent finished. The finished threshold (seconds, default 30).
- **Launch at login:** Tauri autostart plugin, off by default.
- **Claude folder:** override, default `~/.claude`.
- **Watcher mode:** shown only when in polling fallback.
- Settings are stored as JSON in the app data folder.

### Menu bar icon

- A template icon plus the needs-you count (hidden at 0).
- Dropdown:
  - Needs-you agents first, then working agents, up to 10 of each, as "● title — project".
  - Clicking an agent opens the main window on it.
  - Then: Open Agent Viewer, Settings…, Quit.

### Notifications

- Clicking a notification opens the main window on that agent.
- Notifications are grouped per agent; a new one replaces the previous one for that agent.

## 6. Hooks and notifications

### Hook install

1. Read `~/.claude/settings.json` (a missing file is treated as `{}`). If it is not valid JSON, refuse with a message and write nothing.
2. Back it up to `~/.claude/settings.json.agent-viewer-backup-<YYYYMMDD-HHMMSS>`.
3. Add one entry under `hooks.Notification` and one under `hooks.Stop`:
   `{"matcher": "", "hooks": [{"type": "command", "command": "\"<App>/Contents/MacOS/av\" hook --agent-viewer", "timeout": 5}]}`.
   Rules for this step:
   - The `--agent-viewer` marker identifies the app's own entries.
   - All other keys and hooks are kept, in their original key order (serde_json `preserve_order`).
   - Installing again is a no-op.
4. Write to a temp file in the same folder and rename it over the original.

**Uninstall:** removes only entries whose command contains `--agent-viewer`, and removes event arrays that become empty. It also makes a backup first.

**`hooks_status`:**
- `installed` — both entries are present and the command path exists.
- `notInstalled`.
- `broken` — entries exist but the command path is missing (for example, the app was moved). Settings then offers Repair, which is uninstall followed by install.

### `av hook`

1. Reads the hook JSON from stdin and adds a `receivedAt` timestamp.
2. Appends it as one line to `~/.claude/agent-viewer/events.jsonl` (creating the folder if needed) in a single `write` of at most 4 KB. Oversized fields such as `message` are truncated to fit.
3. Never prints to stdout. Swallows every error and always exits 0.

### Notification rules

The notifier is a pure function: `(previous agent, change, settings, now) → [Notification]`.

| Kind | Fires when | Default |
|---|---|---|
| Needs permission | Status becomes `needsPermission` | on |
| Waiting for input | Status becomes `waitingInput`, unless Finished fired for the same agent in the last 10 s | on |
| Finished | `working` → `idle` or `waitingInput`, and the agent had been `working` for ≥ N s (default 30) | on |
| Error | Status becomes `error` | on |
| Subagent finished | A background subagent goes `running` → `done` | on |

Suppression rules:
- No notifications for the initial load, or for an agent first seen already in that status.
- At most one notification per agent per kind per 10 s.
- No notification when the main window is focused and showing that agent.
- `waiting?` never notifies.

If macOS notification permission is denied, Settings says so and links to System Settings → Notifications.

## 7. Error handling

| Situation | Behavior |
|---|---|
| `~/.claude` missing | Empty state shows the path; the watcher waits for it to appear. |
| Registry or transcript file unreadable | That agent becomes `unknown { reason }`; everything else continues. |
| Malformed JSON line (transcript or events) | Skipped and counted; the count is shown in the detail header in debug builds. |
| Transcript truncated or replaced | Re-read from the start. |
| Watcher fails to start or errors | Switch to a 2 s full rescan; Settings shows "polling mode". |
| `ps` fails | Liveness unknown: keep the last known status instead of marking exited. |
| `settings.json` invalid during install | Refuse; show the parse error; nothing written. |
| Tauri command error | Returned as `{ code, message }`; shown as a toast. |

**Write boundaries:** the app writes only to `~/.claude/settings.json` (install, uninstall and repair, always with a backup), `~/.claude/agent-viewer/`, and its own app data folder. It never sends signals or input to any session.

## 8. Testing

### Rust (fixture builders in `tests/common`, never the real `~/.claude`)

- **`tail`:**
  - Appending in arbitrary chunks, including half-written lines, gives the same summary as one full read.
  - Random split points are tried over the M0 fixtures.
  - Truncation and replacement trigger a re-read.
  - Invalid UTF-8 is handled lossily.
- **`engine`:** scripted input sequences checked against expected `Change` lists, with fixed `now`. Cases:
  - registry appears → transcript grows → `Notification` hook → transcript grows (clears it) → `Stop` → exited
  - an open `Agent` tool does not cause `waiting?`
  - stale events are ignored on replay
  - pid reuse
  - context and limit calculation
- **`status`:** table-driven test of the priority order.
- **`hooks`:**
  - Install and uninstall against fixture settings files: missing, empty, with existing hooks, with unrelated keys, and invalid JSON (refused).
  - Installing twice is a no-op.
  - Key order is preserved.
  - A backup is written.
  - Install followed by uninstall gives JSON equal to the original.
  - Status reports `broken` when the path is missing.
- **Notifier:** every row of the rules table, the threshold boundary, the 10 s rate limit, initial-load suppression, the finished/waiting duplicate rule, and the focused-window rule.
- **`av hook`** (assert_cmd):
  - Valid input appends exactly one valid line.
  - Junk stdin or an unwritable path still exits 0.
  - Nothing is printed to stdout.
- **`watch`:** one integration test on a temp folder; a file write must produce an engine input within 2 s.

### Frontend (Vitest + Testing Library)

- The store applies `Change` lists correctly.
- Sorting, grouping, filter chips and their counts.
- A card renders for each status variant; the context bar thresholds work.
- Transcript rendering: tool-call lines, the 4 KB limit, hidden record types.

### Performance

- A fixture generator creates 20 synthetic sessions and keeps appending to them.
- Measured: idle CPU and memory (Activity Monitor), time for a new session to appear and for a status change to show (a timestamped script), and a Rust bench for one appended line.

## 9. M1 exit check (manual)

Run with at least 5 real sessions. The set must include: one on a permission prompt, one idle, one in a git worktree, one with a background subagent, and one in an API error.

- [ ] The window and tray match every terminal.
- [ ] Each notification kind fires once and is correct.
- [ ] Hooks install, uninstall and repair cleanly; `settings.json` is the same as before after uninstall.
- [ ] Performance targets in section 1 are met.
- [ ] The M0 human comparison rows in `docs/m0-findings.md` are filled in.
