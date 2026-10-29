# M0 Data Spike — Findings

Claude Code version tested: **2.1.312** (macOS, local time zone UTC+5:30). Date: 2026-10-29.

## Live comparison

Run with `av list` / `av list --json` (sampled every ~2 s for 40 s) while two real sessions were open.

| Scenario | Terminal shows | `av` shows | Match |
|---|---|---|---|
| This session, running subagents (task reviews) in the background | busy; reviewer running, earlier agents finished | `working`; `└ running general-purpose: Review Task 7…`, 14 earlier agents `done` | yes |
| This session, one 12 s Bash call | busy, Bash running | `working` for 5 s, then `waiting?` until ~2 s after the call ended, then `working` | **no**: false `waiting?` |
| Second session (other repo), busy with a background subagent | not checked by a human | `working`; subagent `running`, 4 earlier `done`; branch `feature/system-settings` | not verified |
| Dead or stale registry file / reused pid | — | covered by unit tests only; no stale files present during the run | n/a |
| Session in a git worktree | — | no live worktree session; path encoding covered by unit tests and the real `…--claude-worktrees-…` folder | n/a |
| Session on a permission prompt | — | **not run, needs a human** | — |
| Idle session | — | **not run, needs a human** | — |
| 5+ concurrent sessions | — | **not run**: only 2 sessions were live | — |

**M0's exit criterion (5+ concurrent sessions match their terminals) is not met yet.** Still to do: open the permission-prompt, idle and worktree sessions, run `cargo run --release -p av-cli -- watch`, and fill in the rows above.

## What the spike settled

- **`waiting?` does not separate permission prompts from long tool calls.** A 12 s Bash call showed as `waiting?` after 5 s. The permission side was not tested, but the heuristic cannot tell the two apart. Hooks are needed in M2, as the PRD expected.
- **Subagent `Done` detection is reliable.** Of 93 subagent transcripts on disk, 92 contain a `SubagentHandback` tool_use, and all 93 have either a handback or `end_turn`. In the live run, every finished subagent showed `done` and the one still running showed `running`.
- **Registry `status` flip speed: not measured.** It needs an idle transition, which can't be watched from inside the session doing the work. Measure it during the human run.
- **`procStart` is written in UTC, but `ps -o lstart` prints local time.** Before the fix, every session read as dead on this machine. `ps` now runs with `TZ=UTC LC_ALL=C`. If Claude Code ever switches to local time, liveness breaks the other way.
- **API errors are not `system` records with `level: "error"`.** They are synthetic `assistant` records with `isApiErrorMessage: true`, an `error` field (for example `authentication_failed`) and `model: "<synthetic>"`. No `level: "error"` system record exists in local history. `last_is_error` checks both shapes. A `system` record with `subtype: "turn_duration"` follows every turn, including right after an API error, so system records other than `level: "error"` must not clear the flag; this is now handled and tested. `turn_duration` is also a useful turn-end signal for M1.
- **A foreground `Agent` tool call stays open while the subagent runs**, so the parent will show `waiting?` for the whole run. Not seen live: all 101 local subagent metas are `requestShape: background`. M1 should exclude `Agent` from the pending-tool heuristic.
- **TOKENS is dominated by cache reads** (for example 10.3M shown, about 10.07M of it `cache_read`). M1 must decide what the column means (for example input+output, or show the parts).
- **The JSON shape is inconsistent.** It mixes camelCase (registry entry) and snake_case (summary), and `AgentStatus` serializes as `"Working"` but `{"Unknown":"x"}`. Freeze the IPC shape before M1.
- **A missing `procStart` falls back to "pid is running".** Every real registry file has the field, so this is unlikely to matter.
- **`gitBranch` is `"HEAD"`** in a repo with no commits yet. Show it as-is.
- **Scan cost:** a release build takes 0.3–0.5 s wall time per full scan, re-reading about 16 MB of transcripts (two sessions with about 6 MB of subagents each). That is fine at a 1 s poll for M0. M1 should read transcripts incrementally (remember the byte offset and summary state per file) before more sessions are added.

## Record types and fields not in the plan

These were all ignored without errors.

- Transcript record types: `last-prompt`, `mode`, `permission-mode`, `atis-latch`, `attachment`, `file-history-snapshot`, `api-request-shape`, `api-request-blob`, `api-request`, `tools-used`, `queue-operation`, `agent-name`.
- `system` subtypes: `turn_duration`, `away_summary`, `compact_boundary`, `informational` (levels `notice`/`warning`), `bridge_status`, `local_command`, `stop_hook_summary`.
- Registry fields: `peerProtocol`, `peerFeatures`, `pidDomain`, `messagingSocketPath`, `nameSource`, `nameSince`, `statusUpdatedAt`.
- Worth a look for M1/M2: `permission-mode` (could show plan/auto mode), `agent-name` (a user-set name), `statusUpdatedAt` (how long a session has been in its current status), and `turn_duration`.
