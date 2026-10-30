# M1 Desktop App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri 2 macOS app that shows every local Claude Code session live, with subagents, transcript detail, notifications, a menu bar badge and a hook installer.

**Architecture:** `av-core` gains a pure incremental `engine` (inputs → `Change` lists) fed by a `watch` thread; the Tauri shell runs that thread, emits changes to a React/Zustand UI, and runs a pure `notifier` for macOS notifications and the tray. Hooks write JSON lines through `av hook` into `~/.claude/agent-viewer/events.jsonl`, which the same watcher sees.

**Tech Stack:** Rust (workspace, edition 2021), serde/serde_json (`preserve_order`), chrono, `notify` + `notify-debouncer-mini`, Tauri 2 (+ plugins notification, autostart, opener, window-state), React + TypeScript + Vite, Tailwind CSS v4 (`@tailwindcss/vite`), Zustand, Vitest + Testing Library (jsdom).

**Spec:** `docs/superpowers/specs/2026-10-30-m1-desktop-app-design.md`

## Global Constraints

- macOS only (this machine: Intel x86_64, macOS 15). Rust at `$HOME/.cargo/bin` — prefix commands with `export PATH="$HOME/.cargo/bin:$PATH"`. Node 22 via nvm, **npm** (no pnpm/bun).
- View-only: never send signals, keystrokes or messages to any session.
- Write boundaries: only `~/.claude/settings.json` (hook install/uninstall/repair, always with a backup first), `~/.claude/agent-viewer/`, and the app's data folder. Tests never touch the real `~/.claude`; they use `tempfile` dirs and the fixture builders in `crates/av-core/tests/common/mod.rs`.
- IPC types serialize camelCase; enums are internally tagged: `Status` and `TranscriptEntry` with `"kind"`, `Change` with `"type"`.
- Status priority (first match wins): `exited` > `needsPermission` > `error` > `waitingInput` > `maybeWaiting` (hooks not installed only; displayed "waiting?") > `working` > `idle` > `unknown`.
- `MAYBE_WAITING_AFTER_SECS = 5`; an open `Agent` tool never makes a session `maybeWaiting`.
- Context limit: 1,000,000 if the model id ends with `[1m]` or any call used > 200,000; else 200,000; `null` if the model is unknown.
- Needs-you count = `needsPermission` + `waitingInput` + `error`. `maybeWaiting` never counts and never notifies.
- Notifications: finished threshold default 30 s; ≤ 1 per agent per kind per 10 s; none for the initial load or for agents first seen in a status; none when the main window is focused on that agent.
- Hook command: `"<dir>/av" hook --agent-viewer`, `timeout: 5`, under `hooks.Notification` and `hooks.Stop` with `"matcher": ""`. Marker substring: `--agent-viewer`. Backup name: `settings.json.agent-viewer-backup-<YYYYMMDD-HHMMSS>`.
- Events file: `<claude_dir>/agent-viewer/events.jsonl`; startup replay window 10 min; trimmed to last 1 MB on app start; each `av hook` write is one `write` ≤ 4 KB.
- Transcript view: last 200 records, pages of 200, tool input/result each cut at 4 KB (4096 bytes, on a char boundary).
- Watcher debounce 100 ms; fallback full rescan every 2 s; engine tick every 1 s; liveness (`ps`) re-check at most every 5 s per session, and immediately on registry change.
- Exited agents stay listed for 24 h, then are removed.
- Existing `av list` / `av watch` behavior must not change; all existing tests keep passing. `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` must be clean at every commit.

**Rulings made while planning (deviations the user should know about):**
- Notification click: Tauri 2's desktop notification plugin has no reliable click callback on macOS. Clicking a notification activates the app (macOS default); on activation the app selects the agent of the most recent notification. "Replace previous notification per agent" is dropped; the 10 s rate limit covers the noise.
- Settings is a second Tauri window (`settings` label) loading the same bundle with `#settings`.

## Review Focus

1. **`/clear` or resume in the same terminal** — the registry file for a pid gets a new `sessionId`. Expected: the old agent becomes exited/removed and a new agent appears; no stale transcript state carried over. (Test in Task 8.)
2. **Hook writes racing the reader** — the app reads `events.jsonl` while `av hook` is mid-append. Expected: the partial line is carried and parsed on the next read, never dropped or double-counted. (Test in Task 4.)
3. **Symlinked `~/.claude/settings.json`** (dotfiles setups). Expected: install writes through to the symlink target and the symlink survives. (Test in Task 5.)
4. **App path with spaces** (`/Applications/Agent Viewer.app/...`). Expected: the hook command is quoted and runs. (Test in Task 5.)
5. **One enormous transcript line** (a multi-MB tool result) in the detail view. Expected: the page still loads, the field is cut at 4 KB, earlier records are still reachable. (Test in Task 9.)

---

## File Structure

```
crates/av-core/src/
  model.rs            NEW  IPC types (Agent, Subagent, Status, Context, Tokens, PendingToolView, Change, TranscriptEntry, TranscriptPage)
  transcript.rs       MOD  SummaryState + TranscriptTail (incremental); summarize_transcript wraps it; new summary fields
  status.rs           MOD  + derive_status_v2 → model::Status (old derive_status kept for the CLI)
  liveness.rs         MOD  + ProcessProbe::check (tri-state)
  hooks/mod.rs        NEW  pub mod events; pub mod install;
  hooks/events.rs     NEW  HookEvent parsing, EventsTail, trim_events_file, write_event
  hooks/install.rs    NEW  install / uninstall / hooks_status for settings.json
  view.rs             NEW  build_agent / build_subagent / context_limit / pick_title
  engine.rs           NEW  Engine: inputs → Vec<Change>
  transcript_view.rs  NEW  read_page → TranscriptPage
  watch.rs            NEW  spawn_watcher (notify + debouncer) → Input
  notifier.rs         NEW  Notifier + NotifySettings (pure)
crates/av-cli/src/main.rs   MOD  + `av hook`
apps/desktop/               NEW  Tauri app (src-tauri is a workspace member)
  scripts/build-sidecar.sh
  src-tauri/src/{main.rs,lib.rs,runtime.rs,commands.rs,tray.rs,settings.rs}
  src/{main.tsx,App.tsx,types.ts,store.ts,lib/view.ts,lib/format.ts,components/*.tsx,__tests__/*.test.ts(x)}
scripts/perf-fixture.sh     NEW  synthetic 20-session generator
docs/m1-exit-check.md       NEW  manual checklist
```

---

### Task 1: IPC model types

**Files:**
- Create: `crates/av-core/src/model.rs`
- Modify: `crates/av-core/src/lib.rs` (add `pub mod model;`)
- Test: `crates/av-core/tests/model.rs`

**Interfaces:**
- Produces (all `#[derive(Debug, Clone, PartialEq, Serialize)]`, `#[serde(rename_all = "camelCase")]`; timestamps are `DateTime<Utc>`):
  - `Agent { id: String, pid: u32, cwd: String, project_name: String, git_branch: Option<String>, title: String, entrypoint: String, model: Option<String>, status: Status, status_since: DateTime<Utc>, started_at: DateTime<Utc>, last_activity: Option<DateTime<Utc>>, context: Context, tokens: Tokens, pending_tool: Option<PendingToolView>, subagents: Vec<Subagent>, transcript_path: Option<String>, skipped_lines: usize }`
  - `Subagent { id: String, agent_type: String, description: String, background: bool, status: SubagentState, last_activity: Option<DateTime<Utc>>, tokens: Tokens, pending_tool: Option<PendingToolView>, transcript_path: String }`
  - `enum SubagentState { Running, Done, Stopped }` → `"running" | "done" | "stopped"` (`rename_all = "lowercase"`)
  - `enum Status` `#[serde(tag = "kind", rename_all = "camelCase")]`: `Exited, NeedsPermission { tool: Option<String> }, Error { kind: Option<String> }, WaitingInput, MaybeWaiting, Working, Idle, Unknown { reason: String }`
  - `Status::needs_you(&self) -> bool` (true for NeedsPermission, WaitingInput, Error); `Status::is_busy(&self) -> bool` (Working, MaybeWaiting, NeedsPermission)
  - `Context { used: Option<u64>, limit: Option<u64> }`, `Tokens { input: u64, output: u64, cache_write: u64, cache_read: u64 }`, `PendingToolView { name: String, since: DateTime<Utc> }`
  - `enum Change` `#[serde(tag = "type", rename_all = "camelCase")]`: `AgentAdded { agent: Agent }, AgentUpdated { agent: Agent }, AgentRemoved { id: String }`
  - `enum TranscriptEntry` `#[serde(tag = "kind", rename_all = "camelCase")]`, fields camelCase: `UserPrompt { text: String, timestamp: Option<DateTime<Utc>> }, AssistantText { text, timestamp }, Thinking { text, timestamp }, ToolCall { id: String, name: String, summary: String, input: String, result: Option<String>, is_error: bool, timestamp }`
  - `TranscriptPage { entries: Vec<TranscriptEntry>, start_offset: u64, end_offset: u64 }`

- [ ] **Step 1: Write failing serialization tests** in `tests/model.rs`:
  - `status_is_kind_tagged`: `to_value(Status::NeedsPermission{tool:Some("Bash".into())})` == `json!({"kind":"needsPermission","tool":"Bash"})`; `to_value(Status::MaybeWaiting)` == `json!({"kind":"maybeWaiting"})`; `Unknown{reason:"x"}` == `json!({"kind":"unknown","reason":"x"})`.
  - `change_is_type_tagged`: `AgentRemoved{id:"a"}` == `json!({"type":"agentRemoved","id":"a"})`.
  - `agent_fields_are_camel_case`: a built `Agent` serializes with keys `projectName`, `statusSince`, `pendingTool`, `transcriptPath`, `skippedLines`, and `tokens.cacheWrite`; no key contains `_`.
  - `needs_you_set`: exactly NeedsPermission, WaitingInput, Error return true.
  - `tool_call_entry_shape`: `ToolCall{..}` serializes `kind:"toolCall"` and `isError`.
- [ ] **Step 2: Run** `cargo test -p av-core --test model` → FAIL (module missing).
- [ ] **Step 3: Implement** `model.rs` per Interfaces; register in `lib.rs`.
- [ ] **Step 4: Run** `cargo test -p av-core --test model` → PASS; `cargo test --workspace` → all pass.
- [ ] **Step 5: Commit** `git commit -m "feat(core): frozen IPC model types"`

---

### Task 2: Incremental transcript tail

**Files:**
- Modify: `crates/av-core/src/transcript.rs`
- Test: `crates/av-core/tests/transcript.rs` (existing tests must still pass), `crates/av-core/tests/tail.rs` (new)
- Modify: `crates/av-core/tests/common/mod.rs` (add builders below)

**Interfaces:**
- Produces:
  - `TranscriptSummary` gains: `agent_name: Option<String>` (from `{"type":"agent-name","agentName":…}`), `context_used: Option<u64>` (input + cache_creation + cache_read of the last non-synthetic assistant usage), `max_context_used: u64`, `turn_ended_at: Option<DateTime<Utc>>` (timestamp of the latest `system` record with `subtype:"turn_duration"`), `last_error_kind: Option<String>` (the `error` field of the latest API-error assistant record; cleared together with `last_is_error`).
  - `pub struct SummaryState` (private fields: the M0 locals `ai_title`, `first_prompt`, `open`, `usage_by_msg`, plus the summary) with `pub fn new() -> Self`, `pub fn apply_line(&mut self, line: &str)` and `pub fn summary(&self) -> TranscriptSummary` (computes title, pending tool, token sums).
  - `pub struct TranscriptTail` with `pub fn new(path: PathBuf) -> Self`, `pub fn read_new(&mut self) -> std::io::Result<bool>` (true if any complete line was applied or a reset happened), `pub fn summary(&self) -> TranscriptSummary`, `pub fn offset(&self) -> u64`, `pub fn path(&self) -> &Path`.
  - `summarize_transcript(path)` keeps its signature and becomes `TranscriptTail::new(path).read_new()?` + `summary()`.
- Algorithm (`read_new`): stat the file; if its inode (`std::os::unix::fs::MetadataExt::ino`) differs from the stored one or its length < offset → reset to a fresh `SummaryState`, offset 0, empty carry. Seek to offset, read to EOF, prepend the carried bytes, split on `\n`; apply every complete line (lossy UTF-8, trimmed, empty skipped); keep the trailing incomplete bytes as the new carry; offset = file length.
- New common builders: `agent_name(name: &str) -> String`, `assistant_text_cached(msg_id, i, o, cache_write, cache_read, model, ts) -> String`, `assistant_api_error_kind(kind: &str, ts) -> String`.

- [ ] **Step 1: Write failing tests** in `tests/tail.rs`:
  - `chunked_appends_equal_full_read`: build a 12-line transcript from the existing builders (prompt, tool_use, tool_result, text, turn_duration, api error, agent_name). For every split point `k` in `0..=bytes.len()` step 7, write `bytes[..k]`, `read_new`, append `bytes[k..]`, `read_new`; assert `tail.summary() == summarize_transcript(full)`.
  - `half_line_is_carried`: write a line without its `\n`, `read_new` → summary unchanged from empty (`title == None`); append `"\n"`, `read_new` → title set.
  - `truncation_resets`: read a file, then overwrite it with a shorter different transcript; `read_new` → summary equals the new file's full summary.
  - `replacement_resets`: delete and recreate the path (new inode) with the same length; summary equals the new content.
  - `context_used_is_last_call`: two assistant records with (input 10, cache_write 20, cache_read 170) then (5, 0, 300); `context_used == Some(305)`, `max_context_used == 305`.
  - `agent_name_and_turn_end`: `agent_name == Some("api contract")`, `turn_ended_at == Some(ts of turn_duration)`.
  - `error_kind`: after `assistant_api_error_kind("authentication_failed", …)`, `last_error_kind == Some("authentication_failed")`; after a following user prompt it is `None`.
- [ ] **Step 2: Run** `cargo test -p av-core --test tail` → FAIL.
- [ ] **Step 3: Implement** — move the per-line body of the M0 loop into `SummaryState::apply_line` unchanged except for the new fields; implement `TranscriptTail` per the algorithm.
- [ ] **Step 4: Run** `cargo test --workspace` → all pass (M0 transcript tests unchanged).
- [ ] **Step 5: Commit** `git commit -m "feat(core): incremental transcript tail"`

---

### Task 3: Status v2 and tri-state liveness

**Files:**
- Modify: `crates/av-core/src/status.rs`, `crates/av-core/src/liveness.rs`
- Test: `crates/av-core/tests/status_v2.rs`, `crates/av-core/tests/liveness.rs`

**Interfaces:**
- Consumes: `model::Status`, `TranscriptSummary` (Task 2).
- Produces:
  - `pub enum HookSignal { Permission { tool: Option<String> }, WaitingInput }` and `pub struct ActiveHook { signal: HookSignal, at: DateTime<Utc> }` (pub fields) — in `status.rs`.
  - `pub struct StatusInputs<'a> { alive: bool, registry_status: &'a str, summary: &'a TranscriptSummary, hook: Option<&'a ActiveHook>, stopped_at: Option<DateTime<Utc>>, hooks_installed: bool, now: DateTime<Utc> }`
  - `pub fn derive_status_v2(i: &StatusInputs) -> model::Status` — rows of the Global Constraints priority. `waitingInput` = hook WaitingInput, or registry `"idle"` with `summary.turn_ended_at.is_some() || stopped_at.is_some()`. `idle` = registry `"idle"` otherwise. `maybeWaiting` = `!hooks_installed` && registry `"busy"` && pending tool name != `"Agent"` && open > 5 s. Registry status other than busy/idle → `Unknown { reason: <that string> }`. Error → `Error { kind: summary.last_error_kind.clone() }`.
  - `ProcessProbe` gains `fn check(&self, pid: u32) -> Result<Option<String>, ProbeError> { Ok(self.proc_start(pid)) }` (default). `SystemProbe::check` returns `Err(ProbeError)` when `ps` cannot be spawned; `ps` exiting non-zero (no such pid) is `Ok(None)`.
  - `pub fn check_alive(entry: &SessionEntry, probe: &dyn ProcessProbe) -> Option<bool>` — `None` when the probe errors; otherwise the M0 `is_alive` rule. `is_alive` stays and maps `None` → `false`.
- [ ] **Step 1: Write failing tests**:
  - `status_v2.rs` table test `priority_table` with one row per status, including: dead + hook permission → Exited; alive + permission hook + api error → NeedsPermission{tool}; alive + error + idle → Error{kind}; idle + turn ended → WaitingInput; idle, no turn ended, no stop → Idle; busy + Bash open 6 s + hooks off → MaybeWaiting; same with hooks on → Working; busy + `Agent` open 60 s + hooks off → Working; busy + Bash open 4 s → Working; registry `"weird"` → Unknown{reason:"weird"}.
  - `liveness.rs` `probe_error_is_unknown`: a fake probe whose `check` returns `Err` → `check_alive` is `None`; `is_alive` is `false`.
- [ ] **Step 2: Run** `cargo test -p av-core --test status_v2 --test liveness` → FAIL.
- [ ] **Step 3: Implement** per Interfaces; leave M0 `derive_status`/`AgentStatus` untouched (the CLI uses them).
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(core): status v2 with hook signals and tri-state liveness"`

---

### Task 4: Hook events file

**Files:**
- Create: `crates/av-core/src/hooks/mod.rs`, `crates/av-core/src/hooks/events.rs`
- Modify: `crates/av-core/src/lib.rs`
- Test: `crates/av-core/tests/hook_events.rs`

**Interfaces:**
- Produces (`av_core::hooks::events`):
  - `pub const EVENTS_REL: &str = "agent-viewer/events.jsonl";` `pub fn events_path(claude_dir: &Path) -> PathBuf`
  - `pub enum HookKind { Notification(HookSignal), Stop }` ; `pub struct HookEvent { session_id: String, kind: HookKind, received_at: DateTime<Utc> }`
  - `pub fn classify_notification(notification_type: Option<&str>, message: &str) -> Option<HookSignal>` — `notification_type` `"permission_prompt"` → Permission, `"idle_prompt"` → WaitingInput, any other present value → None; when absent, message containing `"needs your permission"` → Permission with tool = the word after `"permission to use "` (trimmed of punctuation), containing `"waiting for your input"` → WaitingInput, else None.
  - `pub fn parse_event_line(line: &str) -> Option<HookEvent>` — reads `session_id`, `hook_event_name` (`"Notification"` | `"Stop"`), `notification_type`, `message`, `receivedAt` (RFC 3339). Anything else → None.
  - `pub fn write_event(path: &Path, stdin_json: &str, now: DateTime<Utc>) -> std::io::Result<()>` — parse stdin as JSON object (invalid → `{"raw": <first 512 chars>}`), insert `receivedAt`, serialize to one line; if > 4095 bytes, truncate the `message` field (char boundary) until it fits, else drop to `{session_id, hook_event_name, notification_type, receivedAt}`; create parent dir; open with `append(true)`; one `write_all` of line + `\n`.
  - `pub struct EventsTail` with `new(path: PathBuf) -> Self` and `read_new(&mut self) -> std::io::Result<Vec<HookEvent>>` — same offset/inode/carry rules as `TranscriptTail` (reuse a shared private helper `read_appended(path, &mut TailPos) -> io::Result<(Vec<String>, bool /*reset*/)>` extracted into `crates/av-core/src/tailio.rs`, and make Task 2's `TranscriptTail` use it too).
  - `pub fn trim_events_file(path: &Path, max_bytes: u64) -> std::io::Result<()>` — if larger, keep the last `max_bytes` starting after the first `\n` in that window; atomic temp+rename.
- [ ] **Step 1: Write failing tests**:
  - `classify_by_type` and `classify_by_message` (`"Claude needs your permission to use Bash"` → Permission{tool:Some("Bash")}; `"Claude is waiting for your input"` → WaitingInput; `"auth_success"` type → None).
  - `write_then_parse_roundtrip`: `write_event` of a Notification payload, then `EventsTail::read_new` yields one event with matching session id and `received_at`.
  - `oversized_message_is_truncated`: a 20 KB `message` → the written line is ≤ 4096 bytes including `\n` and still parses.
  - `junk_stdin_still_writes`: `write_event(p, "not json", now)` → file has one line containing `"raw"`; `parse_event_line` of it → None.
  - `partial_line_is_carried` (Review Focus 2): append half a line, `read_new` → empty; append the rest + `\n` → exactly one event; another `read_new` → empty.
  - `trim_keeps_whole_lines`: 3,000 lines → trim to 1 KB → file ≤ 1024 bytes, every line parses.
- [ ] **Step 2: Run** `cargo test -p av-core --test hook_events` → FAIL.
- [ ] **Step 3: Implement** per Interfaces (including the `tailio` extraction; Task 2 tests must still pass).
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(core): hook events file read/write"`

---

### Task 5: Hook installer

**Files:**
- Create: `crates/av-core/src/hooks/install.rs`
- Modify: `crates/av-core/Cargo.toml` (`serde_json = { version = "1", features = ["preserve_order"] }`)
- Test: `crates/av-core/tests/hook_install.rs`

**Interfaces:**
- Produces (`av_core::hooks::install`):
  - `pub const MARKER: &str = "--agent-viewer";`
  - `pub fn hook_command(av_path: &Path) -> String` → `format!("\"{}\" hook {MARKER}", av_path.display())`
  - `pub enum HooksStatus { Installed, NotInstalled, Broken }` (Serialize, `rename_all = "camelCase"`)
  - `pub struct InstallReport { pub backup: Option<PathBuf>, pub changed: bool }`
  - `pub fn install(settings: &Path, av_path: &Path, now: DateTime<Utc>) -> Result<InstallReport, HookError>`
  - `pub fn uninstall(settings: &Path, now: DateTime<Utc>) -> Result<InstallReport, HookError>`
  - `pub fn hooks_status(settings: &Path) -> HooksStatus` — Installed iff both events have a marker entry and the quoted path in each exists; Broken if any marker entry's path is missing; else NotInstalled.
  - `pub enum HookError { InvalidJson(String), Io(std::io::Error) }` implementing `Display`.
- Rules: missing file = `{}` (no backup, file created). No-op (`changed: false`, no backup, no write) when already installed with the same command. Otherwise back up to `<settings>.agent-viewer-backup-<YYYYMMDD-HHMMSS>` (UTC `now`), modify, write `<target>.tmp` then rename over the **resolved** target (`std::fs::canonicalize` when the path is a symlink) so the symlink survives. Install appends `{"matcher":"","hooks":[{"type":"command","command":<hook_command>,"timeout":5}]}` to `hooks.Notification` and `hooks.Stop`, first removing any existing marker entries (this is also how Repair works). Uninstall removes every inner hook whose `command` contains `MARKER`, then matcher groups left with no hooks, then event arrays left empty, then `hooks` if empty. Output is pretty-printed with 2-space indent.
- [ ] **Step 1: Write failing tests** (all in a tempdir):
  - `install_into_missing_file` → file exists, `hooks_status == Installed` (av path = a real temp file), no backup.
  - `install_keeps_existing` → settings with `{"model":"x","hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"say hi"}]}]},"zeta":1}`; after install: `say hi` still present, top-level key order `model, hooks, zeta` unchanged, backup file exists and equals the original bytes.
  - `install_twice_is_noop` → second call `changed == false`, file bytes unchanged, only one backup.
  - `uninstall_restores_meaning` → `serde_json::Value` after install+uninstall == original `Value`.
  - `invalid_json_refused` → `Err(InvalidJson)`, file bytes unchanged, no backup.
  - `broken_when_path_missing` → install with av path, delete that file → `Broken`; install again with a new path → `Installed`, only one marker entry per event.
  - `symlinked_settings_survive` (Review Focus 3) → `settings.json` is a symlink to `dotfiles/settings.json`; after install the link is still a symlink and the target contains the hook.
  - `path_with_spaces_runs` (Review Focus 4) → av path `"<tmp>/Agent Viewer.app/Contents/MacOS/av"` is a script `#!/bin/sh\necho ok > "$(dirname "$0")/ran"`; running `sh -c <hook_command>` creates `ran`.
- [ ] **Step 2: Run** `cargo test -p av-core --test hook_install` → FAIL.
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Run** `cargo test --workspace` → PASS (confirm `preserve_order` did not change any CLI JSON test).
- [ ] **Step 5: Commit** `git commit -m "feat(core): hook installer for settings.json"`

---

### Task 6: `av hook` subcommand

**Files:**
- Modify: `crates/av-cli/src/main.rs`
- Test: `crates/av-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `hooks::events::{write_event, events_path}` (Task 4).
- Produces: `av hook [--agent-viewer] [--claude-dir <dir>]` — reads all stdin, calls `write_event(events_path(dir), stdin, Utc::now())`, ignores every error, prints nothing to stdout or stderr, exits 0. The `--agent-viewer` flag is accepted and ignored (it is only a marker). Directory resolution reuses `resolve_dir`, but a resolution failure is also swallowed.
- [ ] **Step 1: Write failing tests**:
  - `hook_appends_one_line`: `av hook --agent-viewer --claude-dir <tmp>` with a Notification JSON on stdin → exit 0, stdout empty, `<tmp>/agent-viewer/events.jsonl` has exactly one line that `parse_event_line` accepts.
  - `hook_never_fails`: stdin `"garbage"` with `--claude-dir` pointing at a path under a read-only dir (chmod 0o500) → exit 0, stdout and stderr empty.
- [ ] **Step 2: Run** `cargo test -p av-cli` → FAIL.
- [ ] **Step 3: Implement** the subcommand.
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(cli): av hook writes hook events"`

---

### Task 7: View builders

**Files:**
- Create: `crates/av-core/src/view.rs`
- Test: `crates/av-core/tests/view.rs`

**Interfaces:**
- Consumes: `SessionEntry`, `TranscriptSummary`, `SubagentInfo`, `model::*`.
- Produces:
  - `pub fn context_limit(model: Option<&str>, max_used: u64) -> Option<u64>` — `None` if model is None; 1_000_000 if model ends with `"[1m]"` or `max_used > 200_000`; else 200_000.
  - `pub fn pick_title(summary: &TranscriptSummary, registry_name: &str) -> String` — `agent_name` > non-empty `registry_name` > `summary.title` > `"(untitled)"`.
  - `pub fn project_name(cwd: &str) -> String` — last non-empty path segment, or `cwd` itself.
  - `pub fn build_subagent(info: &SubagentInfo, transcript_path: &Path) -> model::Subagent`
  - `pub fn build_agent(entry: &SessionEntry, summary: &TranscriptSummary, subagents: Vec<model::Subagent>, status: model::Status, status_since: DateTime<Utc>, transcript_path: Option<&Path>) -> model::Agent` — `started_at` from `entry.started_at_ms`; `context = Context { used: summary.context_used, limit: context_limit(summary.model, summary.max_context_used) }`.
- [ ] **Step 1: Write failing tests**: `limit_rules` (None model → None; `"claude-opus-5-5"` + 150k → 200k; `"claude-opus-5-5[1m]"` → 1M; `"claude-opus-5-5"` + 250k → 1M); `title_priority` (each fallback level); `project_name_cases` (`"/a/b/"` → `"b"`, `"/"` → `"/"`); `build_agent_maps_fields` (id = session id, tokens mapped, context mapped).
- [ ] **Step 2: Run** `cargo test -p av-core --test view` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(core): agent view builders"`

---

### Task 8: Engine

**Files:**
- Create: `crates/av-core/src/engine.rs`
- Modify: `crates/av-core/src/subagents.rs` (add `pub fn subagent_files(transcript: &Path) -> Vec<(PathBuf, Option<PathBuf>)>` returning `(jsonl, meta)` pairs, and `pub fn subagent_info(jsonl: &Path, meta: Option<&Path>, summary: TranscriptSummary, parent_alive: bool) -> SubagentInfo`; make `read_subagents` use them)
- Test: `crates/av-core/tests/engine.rs`

**Interfaces:**
- Consumes: Tasks 2–4, 7; `registry::read_registry`, `paths::find_transcript`, `liveness::check_alive`.
- Produces:
  - `pub enum Input { Registry, Transcript(PathBuf), Events, Tick, Rescan }`
  - `pub struct Engine` with:
    - `pub fn new(claude_dir: PathBuf, probe: Box<dyn ProcessProbe + Send>, hooks_installed: bool) -> Self`
    - `pub fn load(&mut self, now: DateTime<Utc>) -> Vec<Change>` — first full read: registry, all transcripts and subagents, events replay (only events with `received_at >= now - 10 min` for alive sessions). Returns `AgentAdded` for each agent.
    - `pub fn handle(&mut self, input: Input, now: DateTime<Utc>) -> Vec<Change>`
    - `pub fn set_hooks_installed(&mut self, on: bool, now: DateTime<Utc>) -> Vec<Change>`
    - `pub fn agents(&self) -> Vec<model::Agent>` (sorted by id)
    - `pub fn transcript_path(&self, agent_id: &str, subagent_id: Option<&str>) -> Option<PathBuf>`
- Internal per-session state: `SessionEntry`, `alive: bool`, `last_probe: DateTime<Utc>`, `TranscriptTail`, `HashMap<PathBuf, TranscriptTail>` for subagents, `Option<ActiveHook>`, `stopped_at: Option<DateTime<Utc>>`, `status_since`, `exited_at: Option<DateTime<Utc>>`, and the last emitted `model::Agent`.
- Rules:
  - Sessions are keyed by `sessionId`. `Registry`: re-read the registry; new session ids → add; a session whose registry file vanished or whose pid now maps to another session id → `alive = false`, `exited_at = now` (Review Focus 1); re-probe liveness for every changed entry.
  - `Transcript(path)`: route to the session (or subagent) whose tail has that path; for an unknown path under a known session's subagents dir, add a subagent tail; otherwise ignore.
  - `Events`: read new events from `EventsTail`; Notification → set `ActiveHook`; Stop → clear hook, set `stopped_at = received_at`.
  - Hook clearing: before deriving status, drop the hook if `summary.last_activity > hook.at`, or the process is dead.
  - `stopped_at` is cleared when a new user prompt arrives (`summary.last_activity > stopped_at` and registry is `busy`).
  - `Tick`: re-probe liveness where `now - last_probe >= 5 s`; a probe `None` keeps the previous `alive`; remove agents with `exited_at` older than 24 h (`AgentRemoved`).
  - `Rescan`: same as `Registry` + `read_new` on every tail + `Events`.
  - After any input, rebuild each touched agent via `derive_status_v2` + `build_agent`; update `status_since = now` only when the status `kind` changes; emit `AgentUpdated` only if the rebuilt agent `!=` the last emitted one.
  - A transcript that exists but cannot be read (I/O error other than not-found) → `Unknown { reason: <error text> }`; a missing transcript is not an error (empty summary).
  - At load, sessions already dead get `exited_at` = registry `updatedAt` and are skipped if older than 24 h.
- [ ] **Step 1: Write failing tests** (fixtures + `FakeProbe` with a `check` override; fixed `now`):
  - `load_adds_agents`: two registry files + transcripts → two `AgentAdded`, `agents()` has both.
  - `lifecycle_sequence`: registry busy → `Working`; append tool_use + hook Notification permission (via `write_event` + `Input::Events`) → `NeedsPermission{tool:Some("Bash")}`; append tool_result with a later timestamp + `Input::Transcript` → `Working`; registry idle + Stop event → `WaitingInput`; pid dead + `Tick` 6 s later → `Exited`. Each step returns exactly one `AgentUpdated`.
  - `no_change_no_event`: `Tick` with nothing changed → empty vec.
  - `agent_tool_not_waiting`: hooks off, busy, `Agent` tool open 60 s → `Working`.
  - `unreadable_transcript_is_unknown`: transcript path is a directory → `Unknown{..}`.
  - `stale_events_ignored_on_load`: an event 11 min old for a live session → status not `NeedsPermission`.
  - `session_id_change_same_pid` (Review Focus 1): registry `100.json` switches from session `a` to `b` → `a` becomes `Exited`, `b` is `AgentAdded`.
  - `exited_removed_after_24h`: Tick at `exited_at + 24h + 1s` → `AgentRemoved`.
  - `probe_error_keeps_state`: probe errors on Tick → status stays `Working`.
  - `subagent_appears`: writing a new `subagents/agent-x.jsonl` + `Input::Transcript(that path)` → parent `AgentUpdated` with one running subagent.
- [ ] **Step 2: Run** `cargo test -p av-core --test engine` → FAIL.
- [ ] **Step 3: Implement** per Interfaces and Rules.
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(core): incremental engine"`

---

### Task 9: Transcript page reader

**Files:**
- Create: `crates/av-core/src/transcript_view.rs`
- Test: `crates/av-core/tests/transcript_view.rs`

**Interfaces:**
- Produces:
  - `pub enum PageRequest { Tail, Before(u64), After(u64) }`
  - `pub fn read_page(path: &Path, req: PageRequest, limit: usize) -> std::io::Result<model::TranscriptPage>` — `Tail`: the last `limit` displayable records; `Before(off)`: the `limit` displayable records ending before byte `off`; `After(off)`: all displayable records from `off` to the last complete line. `start_offset`/`end_offset` are the byte offsets of the first returned line's start and the last returned line's end (for `After` with nothing new, both = `off`).
  - `pub const FIELD_MAX: usize = 4096;` `pub fn tool_summary(name: &str, input: &serde_json::Value) -> String` — Bash → `command`; Read/Write/Edit/NotebookEdit → `file_path`; Grep/Glob → `pattern`; Agent → `description`; WebFetch → `url`; otherwise the first string field; first line only, ≤ 120 chars.
- Rules: backward reading in 64 KB blocks from the end until `limit` displayable records are found (a line longer than a block is assembled across blocks). Displayable: user prompts (same rules as M0: not meta, not `<`-prefixed, not tool results), assistant `text` blocks, `thinking` blocks, `tool_use` blocks. A `tool_result` fills `result`/`is_error` of the matching `ToolCall` if that call is in the same page; results are stringified (text blocks joined) and cut at `FIELD_MAX` on a char boundary with a trailing `"…"`; tool `input` is pretty JSON cut the same way. Every other record type is skipped.
- [ ] **Step 1: Write failing tests**: `tail_returns_last_n` (300 prompts, limit 200 → 200 entries, last is prompt 299); `before_pages_back` (`Before(tail_page.start_offset)` → exactly prompts 0–99, 100 entries); `after_returns_new` (append 2 prompts → `After(end_offset)` returns exactly 2); `tool_call_paired` (tool_use + tool_result → one `ToolCall` with `summary == "npm test"` and result set); `hidden_types_skipped` (system, attachment, file-history-snapshot → 0 entries); `huge_line_is_cut` (Review Focus 5: a 5 MB tool_result between two prompts → page has both prompts and the result is ≤ 4096 bytes + `"…"`).
- [ ] **Step 2: Run** `cargo test -p av-core --test transcript_view` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(core): transcript page reader"`

---

### Task 10: File watcher

**Files:**
- Create: `crates/av-core/src/watch.rs`
- Modify: `crates/av-core/Cargo.toml` (add `notify` and `notify-debouncer-mini`, latest mutually compatible major versions)
- Test: `crates/av-core/tests/watch.rs`

**Interfaces:**
- Consumes: `engine::Input`.
- Produces:
  - `pub fn classify_path(claude_dir: &Path, path: &Path) -> Option<Input>` — under `sessions/` and ending `.json` → `Registry`; under `projects/` ending `.jsonl` → `Transcript(path)`; equals `events_path(claude_dir)` → `Events`; else None.
  - `pub struct WatchHandle` (drop stops watching) and `pub fn spawn_watcher(claude_dir: &Path, tx: std::sync::mpsc::Sender<Input>) -> notify::Result<WatchHandle>` — debounced 100 ms, recursive on `claude_dir` (so `sessions/`, `projects/`, `agent-viewer/` appearing later are covered), sends one `Input` per distinct classified path per batch.
- [ ] **Step 1: Write failing tests**: `classify_cases` (pure, five paths); `watcher_sees_append` (tempdir; spawn; append to `projects/-x/s.jsonl`; `rx.recv_timeout(2s)` yields `Transcript` of that path; it must pass locally).
- [ ] **Step 2: Run** `cargo test -p av-core --test watch` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(core): debounced file watcher"`

---

### Task 11: Notifier

**Files:**
- Create: `crates/av-core/src/notifier.rs`
- Test: `crates/av-core/tests/notifier.rs`

**Interfaces:**
- Consumes: `model::{Agent, Change, Status, SubagentState}`.
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] #[serde(rename_all = "camelCase", default)] pub struct NotifySettings { needs_permission: bool, waiting_input: bool, finished: bool, error: bool, subagent_finished: bool, finished_after_secs: u64 }` — `Default`: all true, 30.
  - `pub enum NotifyKind { NeedsPermission, WaitingInput, Finished, Error, SubagentFinished }`
  - `pub struct Notification { pub agent_id: String, pub kind: NotifyKind, pub title: String, pub body: String }` — title = agent title; body: `"Needs permission: <tool>"` / `"Needs permission"`, `"Waiting for your input"`, `"Finished after <m>m <s>s"`, `"API error: <kind>"` / `"API error"`, `"Subagent finished: <description>"`.
  - `pub struct Notifier` with `new() -> Self` and `pub fn on_changes(&mut self, changes: &[Change], settings: &NotifySettings, focused_agent: Option<&str>, now: DateTime<Utc>) -> Vec<Notification>`.
- Rules: keeps last-seen `Agent` per id and `busy_since` per id (set on a transition from non-busy to `is_busy()`, cleared when leaving busy). `AgentAdded` records state, never notifies (this is also the initial-load suppression: the shell passes `load()`'s changes through `on_changes` and discards the result). On `AgentUpdated` with a status kind change:
  - → NeedsPermission / Error: that kind.
  - busy → Idle/WaitingInput with `now - busy_since >= finished_after_secs`: Finished; else → WaitingInput: WaitingInput (skipped if Finished fired for that agent within 10 s).
  - Subagents: for each subagent with `background == true` going `Running → Done`: SubagentFinished.
  - Drop if the kind's setting is off, if `focused_agent == Some(id)`, or if the same (agent, kind) fired within the last 10 s. `MaybeWaiting` never notifies.
- [ ] **Step 1: Write failing tests**: one test per row above, plus `threshold_boundary` (29 s busy → WaitingInput only; 30 s → Finished only), `rate_limit_10s` (two permission transitions 5 s apart → one notification; 11 s apart → two), `added_never_notifies`, `focused_suppresses`, `setting_off_suppresses`, `maybe_waiting_silent`, `foreground_subagent_silent`.
- [ ] **Step 2: Run** `cargo test -p av-core --test notifier` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(core): notification rules"`

---

### Task 12: Tauri app scaffold and runtime

**Files:**
- Create: `apps/desktop/` via `npm create tauri-app@latest` (template `react-ts`, manager `npm`, identifier `dev.agentviewer.app`, product name `Agent Viewer`), then add Tailwind v4 (`@tailwindcss/vite`), Zustand, Vitest, `@testing-library/react`, `@testing-library/jest-dom`, jsdom.
- Create: `apps/desktop/scripts/build-sidecar.sh`, `apps/desktop/src-tauri/src/{runtime.rs,commands.rs,settings.rs}`
- Modify: root `Cargo.toml` (add `apps/desktop/src-tauri` to members), `.gitignore` (`node_modules/`, `apps/desktop/dist/`, `apps/desktop/src-tauri/binaries/`)
- Test: `apps/desktop/src-tauri/tests/settings.rs`

**Interfaces:**
- Consumes: `Engine`, `spawn_watcher`, `read_page`, `hooks::install::*`, `NotifySettings`.
- Produces:
  - `settings.rs`: `#[serde(rename_all="camelCase", default)] pub struct AppSettings { notify: NotifySettings, launch_at_login: bool, claude_dir: Option<String>, hooks_banner_dismissed: bool }`; `pub fn load(dir: &Path) -> AppSettings` (missing/invalid → default); `pub fn save(dir: &Path, s: &AppSettings) -> io::Result<()>` (temp + rename, file `settings.json` in the app data dir).
  - `runtime.rs`: `pub struct Runtime { engine: Mutex<Engine>, notifier: Mutex<Notifier>, ... }` managed as Tauri state. `pub fn start(app: &AppHandle)` — trims the events file to 1 MB, calls `engine.load`, feeds those changes to `notifier.on_changes` (result discarded), spawns the watcher (on error, or when the claude dir does not exist yet: sets `polling = true` and retries `spawn_watcher` on every 2 s rescan until it succeeds, then clears `polling`), and spawns one loop thread that `recv_timeout(1s)` on the input channel: a received input → `handle`; a timeout → `Tick` (and `Rescan` every 2 s when polling). Non-empty changes → `app.emit("agents://changed", &changes)` and → notifier (Task 13 sends them).
  - `commands.rs` (all `#[tauri::command]`, errors as `{ code: String, message: String }`): `list_agents() -> Vec<Agent>`, `get_transcript(agent_id: String, subagent_id: Option<String>, req: PageReq) -> TranscriptPage` where `PageReq` = `#[serde(tag="kind")] { tail, before{offset}, after{offset} }` with `limit` fixed at 200, `get_settings() -> AppSettings`, `set_settings(settings: AppSettings)`, `set_selected_agent(id: Option<String>)`, `watcher_mode() -> "watching" | "polling"`. (Hook commands come in Task 14.)
  - `build-sidecar.sh`: `cargo build --release -p av-cli` and copies `target/release/av` to `src-tauri/binaries/av-$(rustc -vV | sed -n 's/host: //p')`. Wired as part of `beforeDevCommand` and `beforeBuildCommand`; `tauri.conf.json` `bundle.externalBin = ["binaries/av"]`.
- [ ] **Step 1: Scaffold** the app, add deps, add Tailwind, set window title `Agent Viewer`, size 1100×720, min 800×500.
- [ ] **Step 2: Write failing test** `settings_roundtrip_and_defaults`: load from empty dir == `AppSettings::default()` with `notify.finished_after_secs == 30`; save then load is equal; garbage file → default.
- [ ] **Step 3: Implement** `settings.rs`, `runtime.rs`, `commands.rs`, sidecar script; claude dir = `settings.claude_dir` or `$HOME/.claude`.
- [ ] **Step 4: Verify**: `cargo test --workspace` → PASS; `cd apps/desktop && npm run tauri dev` opens a window; in the devtools console `await window.__TAURI__.core.invoke('list_agents')` (enable `withGlobalTauri`) returns the live sessions (this one included). Close it.
- [ ] **Step 5: Commit** `git commit -m "feat(desktop): Tauri scaffold with engine runtime and commands"`

---

### Task 13: Tray, window lifecycle and notifications

**Files:**
- Create: `apps/desktop/src-tauri/src/tray.rs`
- Modify: `apps/desktop/src-tauri/src/{lib.rs,runtime.rs}`, `tauri.conf.json`, capabilities
- Add plugins: `tauri-plugin-notification`, `tauri-plugin-window-state`, `tauri-plugin-autostart`, `tauri-plugin-opener`
- Test: `apps/desktop/src-tauri/tests/tray_menu.rs`

**Interfaces:**
- Produces:
  - `tray.rs`: `pub struct TrayItem { id: String, label: String }` and `pub fn tray_model(agents: &[Agent]) -> (Option<String> /*title*/, Vec<TrayItem>, Vec<TrayItem>)` — title = needs-you count as a string, `None` at 0; needs-you agents then busy agents, ≤ 10 each, sorted by `last_activity` desc, label `"● <title> — <project>"` (title cut to 40 chars). `pub fn rebuild_tray(app: &AppHandle, agents: &[Agent])` builds the menu: the two groups (with a disabled header each, omitted when empty), separator, `Open Agent Viewer`, `Settings…`, `Quit`.
  - Event `ui://select-agent` with payload `String` (agent id), emitted when a tray agent item is clicked or the app is activated after a notification.
- Behavior:
  - Tray icon: a template PNG (`icons/tray-template.png`, 16/32 px, `icon_as_template(true)`); `set_title` with the count.
  - Main window `CloseRequested` → `prevent_close` + `hide`. `RunEvent::Reopen` (Dock click) → show + focus. Track `Focused(bool)` and `set_selected_agent` to compute `focused_agent` for the notifier.
  - Notifications sent with `tauri-plugin-notification` (`title`, `body`); remember the last notified agent id; on the next window/app activation within 60 s, emit `ui://select-agent` for it and clear it.
  - Request notification permission on first launch; expose `notification_permission() -> "granted" | "denied" | "unknown"` command.
  - Menu: app menu gets `Settings…` (⌘,) opening/focusing the `settings` window (URL `index.html#settings`, 560×520).
  - `tauri-plugin-window-state` restores the main window size/position. `set_settings` applies `launch_at_login` via autostart.
- [ ] **Step 1: Write failing test** `tray_model_groups_and_caps`: 12 needs-you + 3 working + 2 idle agents → title `Some("12")`, 10 needs-you items, 3 busy items; zero needs-you → title `None`.
- [ ] **Step 2: Run** `cargo test -p agent-viewer --test tray_menu` (crate name per scaffold) → FAIL.
- [ ] **Step 3: Implement** per Interfaces/Behavior; call `rebuild_tray` after load and after each non-empty change batch; route notifier output to the plugin.
- [ ] **Step 4: Verify**: test passes; `npm run tauri dev`: tray shows count text when a session is waiting; closing the window keeps the tray; Dock click reopens; start a session in another terminal, let it work ≥ 30 s and finish → a "Finished" notification appears.
- [ ] **Step 5: Commit** `git commit -m "feat(desktop): tray, window lifecycle, notifications"`

---

### Task 14: Hook commands in the app

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`, `runtime.rs`

**Interfaces:**
- Consumes: `hooks::install::{install, uninstall, hooks_status, HooksStatus, InstallReport}`, `Engine::set_hooks_installed`.
- Produces commands: `hooks_status() -> HooksStatus`, `install_hooks() -> InstallReport` (also used for Repair), `uninstall_hooks() -> InstallReport`. The av path is `std::env::current_exe()?.parent()/av`. After each call, `engine.set_hooks_installed(status == Installed, now)` and emit its changes. At startup, the runtime sets `hooks_installed` from `hooks_status` before `load`. `HookError::InvalidJson` maps to `{code:"invalidSettings", message}`.
- [ ] **Step 1: Verify by hand** against a scratch settings path: temporarily set `claudeDir` in settings to a scratch dir with a copy of a settings.json; call `install_hooks` from devtools → file has both marker entries, backup exists; `hooks_status` → `installed`; `uninstall_hooks` → entries gone. Restore `claudeDir`.
- [ ] **Step 2: Run** `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` → clean.
- [ ] **Step 3: Commit** `git commit -m "feat(desktop): hook install commands"`

---

### Task 15: Frontend store, list, toolbar and cards

**Files:**
- Create: `apps/desktop/src/types.ts`, `src/store.ts`, `src/lib/view.ts`, `src/lib/format.ts`, `src/components/{Toolbar.tsx,AgentList.tsx,AgentCard.tsx,ContextBar.tsx,SubagentTree.tsx,EmptyState.tsx}`, `src/App.tsx`
- Test: `apps/desktop/src/__tests__/{store.test.ts,view.test.ts,AgentCard.test.tsx}`

**Interfaces:**
- `types.ts`: TypeScript mirrors of Task 1 types (`Agent`, `Subagent`, `Status` as a discriminated union on `kind`, `Change` on `type`, `TranscriptEntry`, `TranscriptPage`, `AppSettings`, `NotifySettings`, `HooksStatus`).
- `store.ts` (Zustand): `agents: Record<string, Agent>`, `selected: {agentId: string, subagentId?: string} | null`, `filter: 'all' | 'needsYou' | 'working' | 'idle' | 'exited'`, `groupBy: 'none' | 'project' | 'entrypoint'`, `showExited: boolean`, `search: string`; actions `setAll(agents)`, `apply(changes: Change[])`, `select(...)` (also invokes `set_selected_agent`), setters.
- `lib/view.ts`: `statusGroup(s: Status): 'needsYou' | 'working' | 'idle' | 'exited'` (maybeWaiting/working → working; unknown → idle), `visibleAgents(state, now: Date): Agent[]` (search over title/projectName/gitBranch, case-insensitive; exited hidden unless `showExited`, and then only exited within 24 h; filter), `sortAgents(a: Agent[])` (group order needsYou, working, idle, exited; then `lastActivity` desc), `groupAgents(a, groupBy): [string, Agent[]][]`, `counts(agents): Record<group, number>`.
- `lib/format.ts`: `fmtTokens(n)` (`182k`, `1.2M`), `fmtDuration(ms)` (`12s`, `3m`, `2h 5m`), `contextLevel(used, limit): 'ok' | 'warn' | 'high'` (> 0.80 warn, > 0.95 high), `statusLabel(s)` (`maybeWaiting` → `"waiting?"`, `needsPermission` → `"needs permission"`, `waitingInput` → `"waiting for you"`).
- `EmptyState`: "No Claude Code sessions running" plus the watched folder (`claudeDir` from `get_settings`, else `~/.claude`).
- Card content and toolbar per spec §5. On mount: `invoke('list_agents')` → `setAll`; `listen('agents://changed')` → `apply`; `listen('ui://select-agent')` → `select`. A 1 s interval re-renders relative times.
- [ ] **Step 1: Write failing tests**: `store.test.ts` (`apply` added/updated/removed; removing the selected agent clears `selected`); `view.test.ts` (sort order, search, exited 24 h window, grouping, counts, `contextLevel` at 0.80/0.81/0.96, `statusLabel(maybeWaiting) === 'waiting?'`); `AgentCard.test.tsx` (renders one card per status kind with its label; pending tool `"Bash · 12s"`; context `"182k / 200k"`; `limit: null` shows `"182k"` only; faded class on maybeWaiting).
- [ ] **Step 2: Run** `cd apps/desktop && npx vitest run` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run` → PASS; `npm run tauri dev` shows live cards matching `av list`.
- [ ] **Step 5: Commit** `git commit -m "feat(ui): agent list, cards, filters and grouping"`

---

### Task 16: Detail panel and transcript view

**Files:**
- Create: `apps/desktop/src/components/{DetailPanel.tsx,TranscriptView.tsx}`
- Test: `apps/desktop/src/__tests__/TranscriptView.test.tsx`

**Interfaces:**
- Consumes: `get_transcript` (Task 12), store `selected`.
- Behavior: header per spec §5 (cwd, pid, model, started, token totals, "Open folder in Finder" via `tauri-plugin-opener` `revealItemInDir(cwd)`). On selection: `get_transcript({kind:'tail'})`; "Load earlier" → `before{offset:startOffset}` and prepend; when an `agents://changed` batch contains the selected agent → `after{offset:endOffset}` and append. Auto-scroll only if the user is within 40 px of the bottom. Thinking entries collapsed by default; tool calls one line `"<name>: <summary>"` that expands to `<pre>` input and result (result styled red when `isError`). No action buttons. In dev builds (`import.meta.env.DEV`) the header also shows `skippedLines`.
- [ ] **Step 1: Write failing tests** (mock `invoke`): renders user/assistant messages; tool call collapsed then expanded shows input and result; thinking collapsed; "Load earlier" calls `before` with the current `startOffset` and prepends.
- [ ] **Step 2: Run** `npx vitest run` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run` → PASS; in `npm run tauri dev`, select this session and watch new records appear live.
- [ ] **Step 5: Commit** `git commit -m "feat(ui): detail panel with live transcript"`

---

### Task 17: Settings window, hooks banner, toasts

**Files:**
- Create: `apps/desktop/src/components/{SettingsView.tsx,HooksBanner.tsx,Toasts.tsx}`
- Modify: `apps/desktop/src/main.tsx` (render `SettingsView` when `location.hash === '#settings'`)
- Test: `apps/desktop/src/__tests__/{SettingsView.test.tsx,HooksBanner.test.tsx}`

**Interfaces:**
- Consumes: `get_settings`, `set_settings`, `hooks_status`, `install_hooks`, `uninstall_hooks`, `notification_permission`, `watcher_mode`.
- Behavior: Settings sections per spec §5 — Hooks (status text; buttons Install / Uninstall / Repair shown per status; last backup path from the last `InstallReport`), Notifications (five toggles + number input "Finished after N seconds of work", min 0), Launch at login, Claude folder (text field, applied on blur; takes effect on restart, note shown), "Polling mode" note when `watcher_mode() === 'polling'`, permission-denied note linking to `x-apple.systempreferences:com.apple.preference.notifications`. Every change calls `set_settings` with the full object. HooksBanner shows in the main window when `hooks_status() !== 'installed'` and `!hooksBannerDismissed`; Install calls `install_hooks`; Dismiss sets `hooksBannerDismissed`. Command errors (`{code,message}`) show as a toast for 5 s.
- [ ] **Step 1: Write failing tests**: toggling "Finished" calls `set_settings` with `notify.finished === false`; banner hidden when installed or dismissed; banner Install calls `install_hooks`; an `invalidSettings` rejection renders a toast with its message.
- [ ] **Step 2: Run** `npx vitest run` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npx vitest run` → PASS; ⌘, opens Settings in `npm run tauri dev`.
- [ ] **Step 5: Commit** `git commit -m "feat(ui): settings, hooks banner, toasts"`

---

### Task 18: Performance fixture, bench, build and exit checklist

**Files:**
- Create: `scripts/perf-fixture.sh`, `crates/av-core/benches/append.rs`, `docs/m1-exit-check.md`, `README.md` (or extend if present)
- Modify: `crates/av-core/Cargo.toml` (`[[bench]] name = "append" harness = false`)

**Interfaces / content:**
- `perf-fixture.sh <dir> [n=20]`: creates `<dir>/sessions/<pid>.json` for `n` fake sessions using the current shell's pid as `pid` and its real `procStart` (so they read as alive), each with a 2 MB transcript; then appends one assistant line per session every 2 s until Ctrl-C. Run the app with Settings → Claude folder = `<dir>`.
- `benches/append.rs`: plain `std::time::Instant` loop (no criterion): a `TranscriptTail` on a 2 MB fixture, append one line, `read_new`; print the mean of 1,000 iterations; the bench `assert!`s mean < 5 ms.
- `docs/m1-exit-check.md`: the spec §9 checklist plus measured results table (new session appear time, status change time, idle CPU, memory, bench mean).
- README: how to run (`npm install`, `npm run tauri dev`), build (`npm run tauri build` → `apps/desktop/src-tauri/target/release/bundle/macos/Agent Viewer.app`; ad-hoc sign with `codesign --force --deep -s - "<app>"` if notifications do not appear), and the hooks note.
- [ ] **Step 1: Run** `cargo bench -p av-core --bench append` → prints mean, PASS (< 5 ms).
- [ ] **Step 2: Build** `cd apps/desktop && npm run tauri build` → `.app` produced; launch it; confirm `Contents/MacOS/av` exists.
- [ ] **Step 3: Measure** with the fixture (20 sessions): Activity Monitor CPU (idle, appends paused) < 2%, memory < 300 MB; record in `docs/m1-exit-check.md`.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `npx vitest run` → all clean.
- [ ] **Step 5: Commit** `git commit -m "chore: perf fixture, bench, build docs and M1 exit checklist"`

The human-only parts of the exit check (5+ real sessions, each notification kind, M0 comparison rows) are left for the user and listed in `docs/m1-exit-check.md`.
