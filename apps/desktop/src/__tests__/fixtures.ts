import type { Agent, Status } from "../types";

export const NOW = new Date("2026-10-30T12:00:00Z");
export const ago = (ms: number) => new Date(NOW.getTime() - ms).toISOString();

export function mkAgent(over: Partial<Agent> = {}): Agent {
  return {
    id: "a1",
    pid: 100,
    cwd: "/tmp/proj",
    projectName: "proj",
    gitBranch: "main",
    title: "Fix bug",
    entrypoint: "cli",
    model: "claude-opus",
    status: { kind: "working" },
    statusSince: ago(180_000),
    startedAt: ago(3_600_000),
    lastActivity: ago(1000),
    context: { used: null, limit: null },
    tokens: { input: 0, output: 0, cacheWrite: 0, cacheRead: 0 },
    pendingTool: null,
    subagents: [],
    transcriptPath: null,
    skippedLines: 0,
    ...over,
  };
}

export const withStatus = (status: Status, over: Partial<Agent> = {}) =>
  mkAgent({ status, ...over });
