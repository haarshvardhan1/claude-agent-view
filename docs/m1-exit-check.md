# M1 Exit Check

Date: 2026-10-30. Machine: Intel x86_64 MacBook, macOS 15. Build: release (`npm run tauri build`), ad-hoc signed.

## Checklist (spec §9)

Run with at least 5 real sessions. The set must include: one on a permission prompt, one idle, one in a git worktree, one with a background subagent, and one in an API error.

- [ ] The window and tray match every terminal. **Needs a human.**
- [ ] Each notification kind fires once and is correct. **Needs a human.**
- [ ] Hooks install, uninstall and repair cleanly; `settings.json` is the same as before after uninstall. **Needs a human** on the real `~/.claude/settings.json`. Automated coverage: install/uninstall/repair round-trips, backups, symlinked settings, paths with spaces (`crates/av-core/tests/hook_install.rs`, `apps/desktop/src-tauri/tests/hooks_commands.rs`).
- [x] Performance targets in section 1 are met, measured with the synthetic fixture below (20 sessions). See the caveats under the table. Still worth a look with real sessions.
- [ ] The M0 human comparison rows in `docs/m0-findings.md` are filled in. **Needs a human.**

## Performance results

Setup: `scripts/perf-fixture.sh --idle <dir> 20` (20 live sessions, each with a real `sleep` pid and a 2 MB transcript, 40 MB in total), release app binary from the bundle (`Agent Viewer.app/Contents/MacOS/desktop`) started with `AGENT_VIEWER_CLAUDE_DIR=<dir> AGENT_VIEWER_LOG_CHANGES=1`. CPU and RSS from `ps -o %cpu=,rss= -p <pid>` once a second for 60 s. "Helpers" are the three WebKit processes (WebContent, GPU, Networking) the app's window starts. Latency is from the fixture's write (epoch ms, taken just before the write) to the app's change-batch log line (epoch ms, just before the batch is sent to the UI). Paint time is not included.

| Target (spec §1) | Result | Pass |
|---|---|---|
| New session appears ≤ 2 s | 171–189 ms (4 runs) | yes |
| Status change shows ≤ 1 s | registry `busy` → `idle`: 129–132 ms; transcript API-error line → `error`: 146–158 ms (4 runs) | yes |
| Idle CPU < 2% (20 agents) | app process, steady state: mean 0.02% (max 0.3%) and 1.33% (max 8.8%) in two runs | yes (app process) |
| Memory < 300 MB (20 agents) | app process RSS ~66–69 MB; app + WebKit helpers max 193 MB | yes |
| One appended line < 5 ms | bench mean 0.055–0.075 ms, max 0.57 ms (1,000 iterations, 2 MB transcript; 3 runs) | yes |

Idle CPU detail (mean % of one core over 60 s):

| Run | App process | WebKit helpers | App + helpers | Notes |
|---|---|---|---|---|
| Before engine change, sampled 20–80 s after launch | 1.56 (max 8.6) | 1.34 | 2.90 (max 13.9) | All 20 cards' "since" labels tick every second for the first minute |
| After, sampled 20–80 s after launch | 3.54 (max 9.7) | 4.23 | 7.77 (max 26.3) | Same first-minute effect; someone clicked the hooks banner's Install 9 s before sampling began (see below) |
| After, steady state (sampled 90–150 s after launch) | 0.02 (max 0.3) | 0.27 | 0.29 (max 2.6) | |
| After, steady state, final build | 1.33 (max 8.8) | 1.20 | 2.52 (max 14.1) | ~0% for the first 40 s, then activity in the app and WebContent together, starting when someone used the window (hooks were installed from the banner at 15:48:28) |

`ps` children: the liveness probe's `ps` runs are separate short processes, so they are not in the app's row in Activity Monitor. Measured with `/usr/bin/time -l` (children's CPU = total − the app's own): about 1.1–1.3% of a core with 20 sessions. On this machine one `ps -p <20 pids>` costs about 45–60 ms of CPU, about the same as 20 single-pid runs.

Engine change (batched liveness probe, commit a37fc5b). Engine alone, no UI: 20 sessions, `Tick` every second for 60 s, two runs each. CPU = everything after the initial load, `ps` children included.

| | Before: one `ps` per session | After: one `ps` per Tick |
|---|---|---|
| Idle CPU, engine + `ps` children | 1.67%, 1.62% | 1.32%, 1.28% |
| Engine's own CPU in Tick | 0.39%, 0.40% | 0.08%, 0.07% |
| Mean wall time per Tick | 16.2 ms, 16.4 ms | 12.5 ms, 12.1 ms |

Memory: peak RSS of the app process (`/usr/bin/time -l`) is 72 MB, with a peak memory footprint of 52 MB.

Caveats:
- "Idle CPU < 2%" holds for the app process, which is what Activity Monitor shows as the app. Counting the WebKit helpers and the `ps` children too, the quiet steady state is about 1.4%. It goes over 2% in the first minute after many statuses change (the cards' second counters re-render every second) and while the window is being used.
- Someone used the app window during three of the four app runs: the hooks banner's Install was clicked (each run's `settings.json` with the hook entries appeared inside the fixture folder, not in `~/.claude`). The only run with no interaction is the 0.02% steady-state run.
- The fixture is synthetic. Real transcripts, subagents and hook events can cost more.

## Needs a human

None of these were checked by the implementer: the app ran only as an unattended process, with no one looking at the screen.

**Real sessions (5+), compared against their terminals**
- [ ] One session on a permission prompt shows `needs permission` (hooks installed) or `waiting?` (no hooks).
- [ ] One idle session shows `idle` / `waiting for input` as its terminal does.
- [ ] One session in a git worktree shows the right project and branch.
- [ ] One session with a background subagent shows the subagent `running`, then `done`.
- [ ] One session in an API error shows `error` with its kind.
- [ ] The M0 rows in `docs/m0-findings.md` are filled in (permission prompt, idle, 5+ concurrent sessions, the unverified second session) and the registry `status` flip speed is noted.

**Notifications** (ad-hoc signed bundle)
- [ ] Each kind fires once with the right title and body: needs permission, waiting for input, finished (≥ 30 s working), error, subagent finished.
- [ ] No notification on launch, for an agent first seen already in that status, or for the agent focused in the main window.
- [ ] Clicking a notification brings the app forward and selects that agent, including when the main window was hidden.

**Menu bar and windows**
- [ ] The tray title shows the needs-you count; the tray menu lists agents and its items work when clicked.
- [ ] Closing the main window hides it (the app keeps running in the menu bar); clicking the Dock icon reopens it.
- [ ] ⌘, opens the Settings window (`#settings` route) in the release build.
- [ ] The "System Settings → Notifications" link in Settings opens System Settings.

**Transcript detail**
- [ ] Auto-scroll follows new lines only when within 40 px of the bottom.
- [ ] "Load earlier" keeps the scroll position stable when older records are prepended.

**Hooks on the real `~/.claude`**
- [ ] Install from the copy of the app you will keep; a backup appears next to `settings.json`.
- [ ] Uninstall leaves `settings.json` identical to before install (`diff` against the backup).
- [ ] Moving the app shows hooks as broken; Repair fixes them.
