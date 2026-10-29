# M0 Data Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A command-line tool `av` that reads `~/.claude/` and lists every live local Claude Code session with its name, task title, status and subagents, so the PRD's file formats and status model are proven before the Tauri app is built.

**Architecture:** A Rust library crate `av-core` holds all discovery and parsing (it becomes the Tauri backend in M1). A thin binary crate `av-cli` calls `av_core::scan()` and prints a table, JSON, or a live-refreshing view. Everything is read-only; the only process interaction is `ps` to check liveness.

**Tech Stack:** Rust (stable, 2021 edition), `serde` + `serde_json`, `chrono`, `clap` (derive), `anyhow`; tests use `tempfile` and `assert_cmd`.

**Spec:** Agent Viewer PRD — https://claude.ai/artifact/Hc3GTdcEagAMUUxWQCwmDb (sections "Data sources", "Milestones → M0").

## Global Constraints

- macOS only; local sessions only; view-only — the tool never writes under `~/.claude/` and never sends signals.
- Claude Code home defaults to `$HOME/.claude`; every entry point accepts an override path (tests never touch the real home).
- Transcripts are read line by line with `BufReader`; never read a whole `.jsonl` into memory (files reach 50 MB+).
- Unknown record types, unknown fields and unparsable lines are skipped and counted, never fatal.
- Task title = latest `ai-title` record's `aiTitle`; else first real user prompt, truncated to 80 chars; else `(new session)`.
- "Waiting for permission" inference threshold: a tool call with no result for more than **5 s** while registry status is `busy`.
- File layout (verified on Claude Code 2.1.312):
  - registry `sessions/<pid>.json`: `pid, sessionId, cwd, name, status ("busy"|"idle"), kind ("interactive"|"bg"), entrypoint, version, procStart, startedAt, updatedAt`
  - transcript `projects/<encoded-cwd>/<sessionId>.jsonl`
  - subagents `projects/<encoded-cwd>/<sessionId>/subagents/agent-<id>.jsonl` + `agent-<id>.meta.json` (`agentType, description, toolUseId, spawnDepth, requestShape`)

## Review Focus

1. **Stale registry file / reused pid** — a `sessions/<pid>.json` whose pid is dead, or now belongs to another process, shows as Exited (hidden by default), never as a live agent. Test: Task 2 `pid_reused_is_not_alive`.
2. **Half-written last line** — Claude Code appends while we read; a truncated final JSON line is skipped and the rest of the summary is intact. Test: Task 4 `truncated_last_line_is_skipped`.
3. **cwd with spaces, dots or a worktree path** — `/Users/x/Agent Viewer App` and `…/.claude/worktrees/foo` resolve to the right project folder, with a fallback search by sessionId. Tests: Task 3.
4. **Brand-new session with no transcript yet** — listed with title `(new session)` and status from the registry, no error. Test: Task 6 `session_without_transcript_is_listed`.
5. **Repeated usage on one message** — Claude Code writes several assistant records per API message carrying the same `usage`; tokens are counted once per `message.id`. Test: Task 4 `tokens_deduped_by_message_id`.

---

## File Structure

```
Cargo.toml                         workspace (members: crates/av-core, crates/av-cli)
crates/av-core/src/lib.rs          pub mod declarations + re-exports
crates/av-core/src/registry.rs     read sessions/*.json → SessionEntry
crates/av-core/src/liveness.rs     ProcessProbe trait, SystemProbe (ps), is_alive
crates/av-core/src/paths.rs        cwd → project folder, transcript lookup
crates/av-core/src/transcript.rs   stream a .jsonl → TranscriptSummary
crates/av-core/src/subagents.rs    read subagents/ → SubagentInfo
crates/av-core/src/status.rs       AgentStatus + derive_status
crates/av-core/src/scan.rs         Agent, ScanResult, scan()
crates/av-core/tests/common/mod.rs fixture builders (fake ~/.claude in a tempdir)
crates/av-core/tests/*.rs          one test file per module
crates/av-cli/src/main.rs          `av list [--json] [--all]`, `av watch`
crates/av-cli/tests/cli.rs         end-to-end against a fixture home
docs/m0-findings.md                spike results fed back into the PRD
```

---

### Task 1: Workspace scaffold and registry reader

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/av-core/Cargo.toml`, `crates/av-core/src/lib.rs`, `crates/av-core/src/registry.rs`, `crates/av-core/tests/common/mod.rs`, `crates/av-core/tests/registry.rs`

**Interfaces:**
- Produces:
  - `pub struct SessionEntry { pub pid: u32, pub session_id: String, pub cwd: String, pub name: String, pub status: String, pub kind: String, pub entrypoint: String, pub version: String, pub proc_start: Option<String>, pub started_at_ms: i64, pub updated_at_ms: i64 }` — `Debug, Clone, Serialize`; serde renames from camelCase; missing optional strings default to `""`.
  - `pub fn read_registry(claude_dir: &Path) -> (Vec<SessionEntry>, Vec<String>)` — entries sorted by `started_at_ms` descending; second value = warnings (one per unreadable file, naming it). Missing `sessions/` dir → empty, no warning.
  - Test helper `common::write_session(home: &Path, pid: u32, session_id: &str, cwd: &str, status: &str)` writing a realistic registry file (`kind: "interactive"`, `entrypoint: "cli"`, `procStart: "Thu Oct 29 12:36:53 2026"`, `name` = `"agent-<pid>"`).

- [ ] **Step 1: Install toolchain and init repo**

Run: `brew install rustup && rustup-init -y && source "$HOME/.cargo/env" && cargo --version` → prints a version. Run `git init` in the project root. `.gitignore` contains `target/`.

- [ ] **Step 2: Write the failing tests** in `crates/av-core/tests/registry.rs`

```rust
#[test] fn reads_entries_newest_first()      // two sessions, startedAt 1000 and 2000 → [2000, 1000]; fields pid, session_id, cwd, status, kind match
#[test] fn bad_file_becomes_warning()        // sessions/999.json = "{not json" → 0 entries, 1 warning containing "999.json"
#[test] fn missing_sessions_dir_is_empty()   // empty tempdir → (vec![], vec![])
#[test] fn unknown_fields_are_ignored()      // extra "peerFeatures":["x"], "jobId":"j" → parses
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p av-core --test registry` → FAIL (unresolved `read_registry`).

- [ ] **Step 4: Implement `read_registry` in `crates/av-core/src/registry.rs`**

Only files ending `.json` in `sessions/`; ignore `.key` files.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p av-core --test registry` → 4 passed.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(core): workspace scaffold and session registry reader"
```

---

### Task 2: Process liveness

**Files:**
- Create: `crates/av-core/src/liveness.rs`, `crates/av-core/tests/liveness.rs`

**Interfaces:**
- Consumes: `SessionEntry` (Task 1)
- Produces:
  - `pub trait ProcessProbe { fn proc_start(&self, pid: u32) -> Option<String>; }` — `None` when the pid is not running.
  - `pub struct SystemProbe;` implementing it via `ps -o lstart= -p <pid>` (trimmed; `None` on non-zero exit or empty output).
  - `pub fn is_alive(entry: &SessionEntry, probe: &dyn ProcessProbe) -> bool` — alive iff probe returns `Some(s)` and, when `entry.proc_start` is `Some(p)`, whitespace-normalized `s == p` (ps pads single-digit days: `"Oct  9"`).
  - Test helper `pub struct FakeProbe(pub HashMap<u32, String>)` in `tests/common/mod.rs`.

- [ ] **Step 1: Write the failing tests** in `crates/av-core/tests/liveness.rs`

```rust
#[test] fn alive_when_start_matches()          // probe {42: "Thu Oct 29 12:36:53 2026"} → true
#[test] fn pid_reused_is_not_alive()           // probe {42: "Fri Oct 30 09:00:00 2026"} → false
#[test] fn dead_pid_is_not_alive()             // empty probe → false
#[test] fn padded_day_matches()                // entry "Thu Oct  9 01:02:03 2026", probe "Thu Oct 9 01:02:03 2026" → true
#[test] fn system_probe_sees_own_process()     // SystemProbe.proc_start(std::process::id()).is_some()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p av-core --test liveness` → FAIL.

- [ ] **Step 3: Implement `ProcessProbe`, `SystemProbe`, `is_alive` in `liveness.rs`**

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p av-core --test liveness` → 5 passed.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(core): pid liveness with reuse check"
```

---

### Task 3: Project folder and transcript lookup

**Files:**
- Create: `crates/av-core/src/paths.rs`, `crates/av-core/tests/paths.rs`

**Interfaces:**
- Produces:
  - `pub fn encode_project_dir(cwd: &str) -> String` — every char that is not ASCII alphanumeric becomes `-`.
  - `pub fn find_transcript(claude_dir: &Path, cwd: &str, session_id: &str) -> Option<PathBuf>` — tries `projects/<encode(cwd)>/<session_id>.jsonl`; if absent, scans `projects/*/<session_id>.jsonl`; else `None`.
  - `pub fn subagents_dir(transcript: &Path) -> PathBuf` — `<transcript without .jsonl>/subagents`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn encodes_spaces()     // "/Users/g2c/Documents/Agent Viewer Application" → "-Users-g2c-Documents-Agent-Viewer-Application"
#[test] fn encodes_worktree()   // "/Users/g2c/repo/.claude/worktrees/zero" → "-Users-g2c-repo--claude-worktrees-zero"
#[test] fn finds_by_encoded_cwd()
#[test] fn falls_back_to_search() // transcript placed under a differently named project folder → found
#[test] fn none_when_missing()
#[test] fn subagents_dir_is_sibling_folder() // ".../abc.jsonl" → ".../abc/subagents"
```

- [ ] **Step 2: Run** `cargo test -p av-core --test paths` → FAIL.
- [ ] **Step 3: Implement the three functions in `paths.rs`.**
- [ ] **Step 4: Run** `cargo test -p av-core --test paths` → 6 passed.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): resolve transcript paths from cwd"`

---

### Task 4: Transcript summarizer

**Files:**
- Create: `crates/av-core/src/transcript.rs`, `crates/av-core/tests/transcript.rs`
- Modify: `crates/av-core/tests/common/mod.rs` (add line builders)

**Interfaces:**
- Produces:
  - `pub struct TokenTotals { pub input: u64, pub output: u64, pub cache_read: u64, pub cache_write: u64 }` — `Default, Serialize`; `pub fn total(&self) -> u64`.
  - `pub struct PendingTool { pub name: String, pub since: DateTime<Utc> }`
  - `pub struct TranscriptSummary { pub title: Option<String>, pub git_branch: Option<String>, pub model: Option<String>, pub last_activity: Option<DateTime<Utc>>, pub pending_tool: Option<PendingTool>, pub last_stop_reason: Option<String>, pub last_is_error: bool, pub handed_back: bool, pub tokens: TokenTotals, pub skipped_lines: usize }` — `Default, Serialize`.
  - `pub fn summarize_transcript(path: &Path) -> std::io::Result<TranscriptSummary>`
  - Test builders in `common`: `user_prompt(text, ts)`, `ai_title(text)`, `assistant_tool_use(msg_id, tool_id, name, ts)`, `assistant_text(msg_id, stop_reason, usage_in, usage_out, ts)`, `tool_result(tool_id, ts)`, `system_error(ts)` — each returns one JSON line shaped like the real records (`type`, `timestamp`, `gitBranch`, `message.{id, model, content, usage, stop_reason}`).

Rules (decided here; the tests pin them):
- `title`: last `ai-title`.`aiTitle`; else first `user` record that is not `isMeta`, whose `message.content` is a string or has a `text` block, and whose text does not start with `<` — first line, cut to 80 chars.
- `git_branch`, `model`, `last_activity`: from the latest record that has them.
- `pending_tool`: `tool_use` blocks are added to an open set by id; a `tool_result` with that `tool_use_id` removes it; a new real user prompt clears the set. Result = most recent remaining.
- `tokens`: per `message.id`, keep the last `usage` seen; sum at the end.
- `last_is_error`: the last record of type `assistant`/`user`/`system` is `system` with `level == "error"`.
- `handed_back`: any `tool_use` named `SubagentHandback`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn ai_title_wins_over_prompt()           // prompt "fix login", then ai_title "Login bug fix" → title "Login bug fix"
#[test] fn falls_back_to_first_prompt()         // "<system-reminder>…" meta line, then "add dark mode\nmore" → "add dark mode"
#[test] fn long_prompt_cut_to_80()              // 200-char prompt → 80 chars
#[test] fn open_tool_is_pending()               // tool_use t1 "Bash" at T → pending_tool name "Bash", since T
#[test] fn answered_tool_is_not_pending()       // tool_use t1 + tool_result t1 → None
#[test] fn tokens_deduped_by_message_id()       // two records msg m1 usage (10 in, 5 out) + msg m2 (1, 1) → input 11, output 6
#[test] fn truncated_last_line_is_skipped()     // valid lines then `{"type":"assist` → skipped_lines 1, title still set
#[test] fn unknown_types_are_ignored()          // {"type":"artifact-autoreact-ledger",…} → skipped_lines 0, no effect
#[test] fn error_as_last_record()               // assistant then system_error → last_is_error true
#[test] fn handback_detected()                  // tool_use name "SubagentHandback" → handed_back true
```

- [ ] **Step 2: Run** `cargo test -p av-core --test transcript` → FAIL.
- [ ] **Step 3: Implement `summarize_transcript` in `transcript.rs`** — `BufReader::lines()`, `serde_json::Value` per line.
- [ ] **Step 4: Run** `cargo test -p av-core --test transcript` → 10 passed.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): stream-summarize session transcripts"`

---

### Task 5: Subagent reader

**Files:**
- Create: `crates/av-core/src/subagents.rs`, `crates/av-core/tests/subagents.rs`

**Interfaces:**
- Consumes: `summarize_transcript`, `TranscriptSummary` (Task 4); `subagents_dir` (Task 3)
- Produces:
  - `pub enum SubagentStatus { Running, Done, Stopped }` — `Serialize`.
  - `pub struct SubagentInfo { pub agent_id: String, pub agent_type: String, pub description: String, pub background: bool, pub status: SubagentStatus, pub summary: TranscriptSummary }`
  - `pub fn read_subagents(transcript: &Path, parent_alive: bool) -> Vec<SubagentInfo>` — one per `agent-<id>.jsonl`, newest `last_activity` first. Missing meta → `agent_type "unknown"`, `description` = agent id. `background` = `requestShape == "background"`.
  - Status: `Done` if `summary.handed_back` or `last_stop_reason == Some("end_turn")`; else `Running` if `parent_alive`; else `Stopped`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn reads_meta_fields()            // meta {agentType:"Explore", description:"Check frontend", requestShape:"background"} → fields match, background true
#[test] fn handback_means_done()
#[test] fn open_work_with_live_parent_is_running()
#[test] fn open_work_with_dead_parent_is_stopped()
#[test] fn missing_meta_uses_agent_id()
#[test] fn no_subagents_dir_is_empty()
```

- [ ] **Step 2: Run** `cargo test -p av-core --test subagents` → FAIL.
- [ ] **Step 3: Implement `read_subagents` in `subagents.rs`.**
- [ ] **Step 4: Run** `cargo test -p av-core --test subagents` → 6 passed.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): read subagent transcripts and metadata"`

---

### Task 6: Status model and `scan()`

**Files:**
- Create: `crates/av-core/src/status.rs`, `crates/av-core/src/scan.rs`, `crates/av-core/tests/status.rs`, `crates/av-core/tests/scan.rs`
- Modify: `crates/av-core/src/lib.rs` (re-export `scan`, `Agent`, `ScanResult`, `AgentStatus`, `SystemProbe`)

**Interfaces:**
- Consumes: everything from Tasks 1–5.
- Produces:
  - `pub enum AgentStatus { Working, MaybeWaiting, Idle, Error, Exited, Unknown(String) }` — `Serialize`; `pub fn label(&self) -> &str` → `"working" | "waiting?" | "idle" | "error" | "exited" | <raw>`.
  - `pub fn derive_status(registry_status: &str, alive: bool, summary: &TranscriptSummary, now: DateTime<Utc>) -> AgentStatus` — first match wins: `!alive` → Exited; `summary.last_is_error` → Error; `"idle"` → Idle; `"busy"` with `pending_tool.since` older than 5 s → MaybeWaiting; `"busy"` → Working; else `Unknown(status)`.
  - `pub struct Agent { pub entry: SessionEntry, pub alive: bool, pub status: AgentStatus, pub title: String, pub summary: TranscriptSummary, pub subagents: Vec<SubagentInfo> }` — `title` resolves the Global Constraints rule including `(new session)`.
  - `pub struct ScanResult { pub agents: Vec<Agent>, pub warnings: Vec<String> }`
  - `pub fn scan(claude_dir: &Path, probe: &dyn ProcessProbe, now: DateTime<Utc>) -> ScanResult` — a transcript read error becomes a warning, not a dropped agent.

- [ ] **Step 1: Write the failing tests**

`tests/status.rs`: one test per rule above, plus `busy_with_fresh_pending_tool_is_working` (pending 2 s → Working) and `unknown_status_passes_through` (`"parked"` → `Unknown("parked")`).

`tests/scan.rs`:
```rust
#[test] fn live_session_full_picture()             // registry busy + transcript with ai_title + 1 subagent; FakeProbe alive → title, status Working, 1 subagent
#[test] fn session_without_transcript_is_listed()  // registry only → title "(new session)", status Idle for "idle"
#[test] fn dead_session_is_exited()                // FakeProbe empty → Exited
```

- [ ] **Step 2: Run** `cargo test -p av-core --test status --test scan` → FAIL.
- [ ] **Step 3: Implement `derive_status` and `scan`.**
- [ ] **Step 4: Run** `cargo test -p av-core` → all pass.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(core): status model and full scan"`

---

### Task 7: `av` command-line tool

**Files:**
- Create: `crates/av-cli/Cargo.toml` (bin name `av`), `crates/av-cli/src/main.rs`, `crates/av-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `av_core::{scan, SystemProbe, ScanResult, Agent}` (Task 6)
- Produces the commands:
  - `av list [--json] [--all] [--claude-dir <path>]` — default hides `Exited` agents. Table columns: `STATUS  NAME  TASK  PROJECT  BRANCH  KIND  AGE  TOKENS`; `PROJECT` = last path component of `cwd`; `AGE` = since `started_at_ms` as `3m` / `2h` / `1d`; `TOKENS` like `182k`. Each subagent on its own line under its parent: `  └ <status> <agent_type>: <description>`. Warnings print to stderr. `--json` prints `ScanResult` as JSON to stdout.
  - `av watch [--all] [--claude-dir <path>]` — clear screen (`\x1b[2J\x1b[H`), print the `list` table, repeat every 1 s until Ctrl-C.

- [ ] **Step 1: Write the failing tests** in `crates/av-cli/tests/cli.rs` (fixture home with dead pids, built with the same helpers copied into this crate's tests or via a `dev-dependency` on a `testkit` feature — implementer's choice)

```rust
#[test] fn list_hides_exited_by_default()   // stdout has header, not the session name
#[test] fn list_all_shows_exited()          // --all → name and "exited" present
#[test] fn json_is_valid()                  // --json --all → serde_json parses; agents.len() == 1
#[test] fn subagent_line_is_indented()      // --all → line starting "  └ " containing the description
```

- [ ] **Step 2: Run** `cargo test -p av-cli` → FAIL.
- [ ] **Step 3: Implement `main.rs` with `clap` derive subcommands `List` and `Watch`.**
- [ ] **Step 4: Run** `cargo test --workspace` → all pass; `cargo run -p av-cli -- list` prints this session's own row.
- [ ] **Step 5: Commit** `git add -A && git commit -m "feat(cli): av list and av watch"`

---

### Task 8: Live validation and findings

**Files:**
- Create: `docs/m0-findings.md`

M0's exit criterion (PRD): output matches what the terminals show for 5+ concurrent sessions.

- [ ] **Step 1: Set up the comparison.** Open 5 Claude Code sessions in different folders (one in a worktree, one launching a background subagent, one left on a permission prompt, one idle, one running a long `sleep 30` Bash call). Run `cargo run -p av-cli -- watch` in another terminal.
- [ ] **Step 2: Record results** in `docs/m0-findings.md` as a table: scenario · terminal shows · `av` shows · match (yes/no).
- [ ] **Step 3: Record what the spike settled**, one line each: does `waiting?` separate permission prompts from long tool calls (expected: no — confirms hooks are needed in M2); is subagent `Done` detection reliable; how fast does the registry `status` flip; any record types or registry fields not in this plan; Claude Code version tested.
- [ ] **Step 4: Commit** `git add docs/m0-findings.md && git commit -m "docs: M0 spike findings"`
