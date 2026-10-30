# Agent Viewer

A macOS menu bar and desktop app that shows every local Claude Code session at a glance: what each one is doing, which ones need you, and what their subagents are up to. It can notify you when a session needs you or finishes. It is view-only: it reads `~/.claude` and never sends input or signals to a session.

- `crates/av-core`: the Rust core (registry, transcripts, liveness, status, engine, watcher, hooks).
- `crates/av-cli`: the `av` command-line tool.
- `apps/desktop`: the Tauri 2 app (Rust shell in `src-tauri`, React + TypeScript + Tailwind UI).

macOS only. You need Rust (stable) and Node 22 with npm.

## Run in dev

```sh
cd apps/desktop
npm install
npm run tauri dev
```

`tauri dev` first builds the `av` sidecar (`scripts/build-sidecar.sh`), then starts Vite and the app.

## Build and ad-hoc sign

```sh
cd apps/desktop
npm run tauri build
```

The app bundle is written to the workspace target folder:

```
target/release/bundle/macos/Agent Viewer.app
```

The `av` helper is bundled at `Agent Viewer.app/Contents/MacOS/av`. The build is unsigned. If notifications do not appear, ad-hoc sign it:

```sh
codesign --force --deep -s - "target/release/bundle/macos/Agent Viewer.app"
codesign -dv "target/release/bundle/macos/Agent Viewer.app"
```

Install hooks only from the copy you will keep (for example after moving it to `/Applications`): the hook command stores the full path to `av`. If you move the app afterwards, Settings shows the hooks as broken and offers Repair.

## Tests and bench

```sh
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo bench -p av-core --bench append   # one appended line on a 2 MB transcript; asserts mean < 5 ms
cd apps/desktop && npx vitest run && npm run typecheck
```

## The `av` CLI

```sh
cargo run --release -p av-cli -- list            # table of live agents
cargo run --release -p av-cli -- list --json     # JSON
cargo run --release -p av-cli -- list --all      # include exited agents
cargo run --release -p av-cli -- watch           # live view, refreshed every second
```

All commands accept `--claude-dir <dir>` (default `~/.claude`).

`av hook` is what the installed Claude Code hooks run. It reads the hook JSON on stdin and appends one line to `<claude-dir>/agent-viewer/events.jsonl`. It never prints and always exits 0. You do not run it by hand.

## Hooks

Without hooks, the app guesses "waiting?" when a tool call has been open for 5 s. With hooks, it knows when a session is on a permission prompt or waiting for input.

- Settings → Hooks → Install writes to `~/.claude/settings.json`. It adds one `Notification` and one `Stop` entry whose command contains `--agent-viewer`, and keeps everything else.
- Before every write it saves a backup next to it: `~/.claude/settings.json.agent-viewer-backup-<YYYYMMDD-HHMMSS>`.
- Uninstall removes only the `--agent-viewer` entries (also with a backup first). Repair is uninstall followed by install.
- Hook events go to `~/.claude/agent-viewer/events.jsonl`. The app trims that file to its last 1 MB on start.

## Pointing the app at another Claude folder

The app reads `~/.claude` by default. Two ways to change that:

- Settings → Claude folder (stored as `claudeDir` in the app's `settings.json` under `~/Library/Application Support/dev.agentviewer.app/`). Applies on restart.
- The `AGENT_VIEWER_CLAUDE_DIR` environment variable. When set and non-empty it wins over the setting, in dev and release builds alike. When unset, it has no effect. It is for fixtures and testing, so the real settings stay untouched:

```sh
AGENT_VIEWER_CLAUDE_DIR=/tmp/fixture "target/release/bundle/macos/Agent Viewer.app/Contents/MacOS/desktop"
```

`AGENT_VIEWER_LOG_CHANGES=1` makes the app log every change batch it sends to the UI to stderr, with an epoch-millisecond timestamp (`agent-viewer: changes t=<ms> added:<id>:<status> ...`). It is off by default and only meant for latency measurements.

## Performance fixture

`scripts/perf-fixture.sh` creates synthetic sessions for performance checks. Each session gets a real `sleep` process as its pid (so it reads as alive) and a ~2 MB transcript.

```sh
scripts/perf-fixture.sh /tmp/av-fixture 20          # 20 sessions, one appended line per session every 2 s
scripts/perf-fixture.sh --idle /tmp/av-fixture 20   # same, but no appends (idle CPU / memory)
scripts/perf-fixture.sh --latency /tmp/av-fixture   # while one of the above runs: new session, status flip, error line
```

Run the app against it with `AGENT_VIEWER_CLAUDE_DIR=/tmp/av-fixture`. Ctrl-C stops the fixture, kills its `sleep` processes and removes the `sessions/` and `projects/` folders it created. The script refuses to write into `~/.claude`.

The M1 results and the manual exit check are in `docs/m1-exit-check.md`.
