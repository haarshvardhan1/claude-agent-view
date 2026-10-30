#!/usr/bin/env bash
# Synthetic Claude Code sessions for Agent Viewer performance checks.
#
#   scripts/perf-fixture.sh [--idle] <dir> [n=20]
#       Creates <dir>/sessions/<pid>.json and a ~2 MB transcript under
#       <dir>/projects/ for n sessions, then appends one assistant line per
#       session every 2 s until Ctrl-C / SIGTERM. With --idle, appends nothing
#       (for idle CPU / memory measurement). Each session's pid is a real
#       `sleep` process started by this script, with its real start time as
#       `procStart`, so the app sees every session as alive.
#
#   scripts/perf-fixture.sh --latency <dir>
#       Against a running fixture in <dir>: adds one new session, then flips
#       session 1's registry status busy -> idle, then appends an API-error
#       line to session 2's transcript, 3 s apart. Prints the epoch-ms time of
#       each write, to compare with the app's change log
#       (AGENT_VIEWER_LOG_CHANGES=1).
#
# Point the app at the fixture with AGENT_VIEWER_CLAUDE_DIR=<dir>.
# Writes only inside <dir>; refuses ~/.claude. On exit it stops its `sleep`
# processes and removes the sessions/ and projects/ folders it created.
set -euo pipefail

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
}

mode=run
case "${1:-}" in
  --idle) mode=idle; shift ;;
  --latency) mode=latency; shift ;;
  -h|--help|"") usage ;;
esac
[ $# -ge 1 ] || usage
dir="$1"
n="${2:-20}"

mkdir -p "$dir"
dir="$(cd "$dir" && pwd -P)"
real_home_claude="$(cd "$HOME" && pwd -P)/.claude"
case "$dir/" in
  "$real_home_claude"/*) echo "refusing to write into $real_home_claude" >&2; exit 1 ;;
esac

now_ms() { perl -MTime::HiRes=time -e 'printf "%d\n", time*1000'; }
iso_now() { perl -MTime::HiRes=time -MPOSIX=strftime -e 'my $t=time; printf "%s.%03dZ\n", strftime("%Y-%m-%dT%H:%M:%S", gmtime($t)), ($t-int($t))*1000'; }
# Same format and normalisation as crates/av-core/src/liveness.rs.
proc_start() { TZ=UTC LC_ALL=C ps -o lstart= -p "$1" | awk '{$1=$1; print}'; }

pids=()
cleanup() {
  trap - INT TERM EXIT
  exec 2>/dev/null # silence bash's "Terminated" job notices
  if [ "${#pids[@]}" -gt 0 ]; then kill "${pids[@]}" || true; wait || true; fi
  if [ "$mode" != latency ]; then rm -rf "$dir/sessions" "$dir/projects"; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

cwd_for() { echo "/perf/fixture/repo-$1"; }
# encode_project_dir: every non-ASCII-alphanumeric char becomes '-'.
proj_for() { echo "$dir/projects/$(cwd_for "$1" | sed 's/[^A-Za-z0-9]/-/g')"; }
sid_for() { printf 'perf0000-0000-4000-8000-%012d' "$1"; }

# write_session <i> <pid> <status>: atomic registry write (temp + rename).
write_session() {
  local i="$1" pid="$2" status="$3" ps_start started
  ps_start="$(proc_start "$pid")"
  started="${STARTED_MS:-$(now_ms)}"
  mkdir -p "$dir/sessions"
  cat > "$dir/sessions/.$pid.json.tmp" <<EOF
{"pid":$pid,"sessionId":"$(sid_for "$i")","cwd":"$(cwd_for "$i")","startedAt":$started,"procStart":"$ps_start","version":"2.1.312","peerProtocol":1,"kind":"interactive","entrypoint":"cli","name":"perf-$i","status":"$status","updatedAt":$started}
EOF
  mv "$dir/sessions/.$pid.json.tmp" "$dir/sessions/$pid.json"
}

# write_transcript <i>: ~2 MB of realistic records (unique message/tool ids).
write_transcript() {
  local i="$1" p
  p="$(proj_for "$i")"
  mkdir -p "$p"
  awk -v sid="$(sid_for "$i")" -v n="$i" 'BEGIN {
    body = ""; for (k = 0; k < 40; k++) body = body "fn line_" k "() { let x = compute(" k "); }\\n";
    printf "{\"type\":\"user\",\"isSidechain\":false,\"timestamp\":\"2026-10-29T13:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"Perf fixture session %d: refactor the parser\"},\"gitBranch\":\"main\",\"sessionId\":\"%s\",\"version\":\"2.1.312\"}\n", n, sid;
    printf "{\"type\":\"ai-title\",\"aiTitle\":\"Perf fixture %d\",\"sessionId\":\"%s\"}\n", n, sid;
    bytes = 0; j = 0;
    while (bytes < 2097152) {
      a = sprintf("{\"type\":\"assistant\",\"timestamp\":\"2026-10-29T13:10:00.000Z\",\"gitBranch\":\"main\",\"message\":{\"id\":\"msg_t%d\",\"model\":\"claude-opus-5-5\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_%d\",\"name\":\"Read\",\"input\":{\"file_path\":\"/perf/src/f%d.rs\"}}],\"stop_reason\":\"tool_use\",\"usage\":{\"input_tokens\":3,\"output_tokens\":40,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":41000}}}", j, j, j);
      r = sprintf("{\"type\":\"user\",\"timestamp\":\"2026-10-29T13:10:01.000Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_%d\",\"content\":\"%s\",\"is_error\":false}]}}", j, body);
      t = sprintf("{\"type\":\"assistant\",\"timestamp\":\"2026-10-29T13:10:02.000Z\",\"gitBranch\":\"main\",\"message\":{\"id\":\"msg_%d\",\"model\":\"claude-opus-5-5\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Read the file; continuing with the next step of the refactor.\"}],\"stop_reason\":\"end_turn\",\"usage\":{\"input_tokens\":12,\"output_tokens\":340,\"cache_creation_input_tokens\":900,\"cache_read_input_tokens\":41000}}}", j, j);
      print a; print r; print t;
      bytes += length(a) + length(r) + length(t) + 3; j++;
    }
  }' > "$p/$(sid_for "$i").jsonl"
}

append_line() {
  local i="$1" seq="$2"
  printf '{"type":"assistant","timestamp":"%s","gitBranch":"main","message":{"id":"msg_live_%s_%s","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"Still working."}],"stop_reason":"end_turn","usage":{"input_tokens":5,"output_tokens":20,"cache_creation_input_tokens":0,"cache_read_input_tokens":42000}}}\n' \
    "$(iso_now)" "$i" "$seq" >> "$(proj_for "$i")/$(sid_for "$i").jsonl"
}

start_sleeper() {
  sleep 100000 &
  pids+=("$!")
  last_pid="$!"
}

pid_of_session() { grep -l "\"sessionId\":\"$(sid_for "$1")\"" "$dir"/sessions/*.json | head -1 | xargs basename | sed 's/\.json$//'; }

if [ "$mode" = latency ]; then
  [ -d "$dir/sessions" ] || { echo "no fixture running in $dir" >&2; exit 1; }
  new=9999
  start_sleeper
  write_transcript "$new"
  t=$(now_ms); write_session "$new" "$last_pid" busy
  echo "latency: new-session $(sid_for "$new") t=$t"
  sleep 3
  pid1="$(pid_of_session 1)"
  t=$(now_ms); STARTED_MS=1793281174333 write_session 1 "$pid1" idle
  echo "latency: status-flip $(sid_for 1) busy->idle t=$t"
  sleep 3
  t=$(now_ms)
  printf '{"type":"system","subtype":"api_error","level":"error","content":"boom","timestamp":"%s"}\n' "$(iso_now)" >> "$(proj_for 2)/$(sid_for 2).jsonl"
  echo "latency: transcript-error $(sid_for 2) t=$t"
  sleep 3
  # Leave the fixture as it was: drop the extra session (the trap stops its pid).
  rm -f "$dir/sessions/$last_pid.json"
  rm -rf "$(proj_for "$new")"
  exit 0
fi

echo "perf-fixture: creating $n sessions in $dir" >&2
for i in $(seq 1 "$n"); do
  start_sleeper
  write_transcript "$i"
  STARTED_MS=1793281174333 write_session "$i" "$last_pid" busy
done
echo "perf-fixture: ready ($(du -sh "$dir/projects" | cut -f1) of transcripts); mode=$mode; Ctrl-C to stop" >&2

seq_no=0
while true; do
  if [ "$mode" = run ]; then
    seq_no=$((seq_no + 1))
    for i in $(seq 1 "$n"); do append_line "$i" "$seq_no"; done
  fi
  sleep 2 &
  wait $! || true
done
