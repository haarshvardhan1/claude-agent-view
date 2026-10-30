import { describe, expect, it } from "vitest";
import {
  counts,
  groupAgents,
  sortAgents,
  statusGroup,
  visibleAgents,
  type ViewState,
} from "../lib/view";
import { contextLevel, fmtDuration, fmtTokens, statusLabel } from "../lib/format";
import type { Agent } from "../types";
import { NOW, ago, mkAgent, withStatus } from "./fixtures";

const view = (agents: Agent[], over: Partial<ViewState> = {}): ViewState => ({
  agents: Object.fromEntries(agents.map((a) => [a.id, a])),
  filter: "all",
  showExited: false,
  search: "",
  ...over,
});

describe("statusGroup", () => {
  it("maps statuses to groups", () => {
    expect(statusGroup({ kind: "maybeWaiting" })).toBe("working");
    expect(statusGroup({ kind: "working" })).toBe("working");
    expect(statusGroup({ kind: "unknown", reason: "x" })).toBe("idle");
    expect(statusGroup({ kind: "needsPermission", tool: null })).toBe("needsYou");
    expect(statusGroup({ kind: "error", errorKind: null })).toBe("needsYou");
    expect(statusGroup({ kind: "waitingInput" })).toBe("needsYou");
    expect(statusGroup({ kind: "exited" })).toBe("exited");
  });
});

describe("sortAgents", () => {
  it("orders by group then lastActivity desc", () => {
    const list = [
      withStatus({ kind: "idle" }, { id: "idle" }),
      withStatus({ kind: "exited" }, { id: "exited" }),
      withStatus({ kind: "working" }, { id: "w-old", lastActivity: ago(9000) }),
      withStatus({ kind: "working" }, { id: "w-new", lastActivity: ago(1000) }),
      withStatus({ kind: "waitingInput" }, { id: "need" }),
    ];
    expect(sortAgents(list).map((a) => a.id)).toEqual([
      "need",
      "w-new",
      "w-old",
      "idle",
      "exited",
    ]);
  });
});

describe("visibleAgents", () => {
  it("searches title, project and branch case-insensitively, skipping null branch", () => {
    const list = [
      mkAgent({ id: "t", title: "Refactor Parser", projectName: "x", gitBranch: null }),
      mkAgent({ id: "p", title: "q", projectName: "ParserKit", gitBranch: null }),
      mkAgent({ id: "b", title: "q", projectName: "x", gitBranch: "feat/PARSER" }),
      mkAgent({ id: "n", title: "q", projectName: "x", gitBranch: null }),
    ];
    const ids = visibleAgents(view(list, { search: "parser" }), NOW).map((a) => a.id);
    expect(ids.sort()).toEqual(["b", "p", "t"]);
  });

  it("hides exited by default, shows within 24h only when showExited", () => {
    const list = [
      withStatus({ kind: "exited" }, { id: "recent", statusSince: ago(3_600_000) }),
      withStatus({ kind: "exited" }, { id: "old", statusSince: ago(25 * 3_600_000) }),
      mkAgent({ id: "live" }),
    ];
    expect(visibleAgents(view(list), NOW).map((a) => a.id)).toEqual(["live"]);
    expect(
      visibleAgents(view(list, { showExited: true }), NOW)
        .map((a) => a.id)
        .sort(),
    ).toEqual(["live", "recent"]);
  });

  it("applies the chip filter", () => {
    const list = [
      withStatus({ kind: "waitingInput" }, { id: "n" }),
      withStatus({ kind: "working" }, { id: "w" }),
      withStatus({ kind: "idle" }, { id: "i" }),
      withStatus({ kind: "exited" }, { id: "e" }),
    ];
    const ids = (f: ViewState["filter"]) =>
      visibleAgents(view(list, { filter: f }), NOW).map((a) => a.id);
    expect(ids("needsYou")).toEqual(["n"]);
    expect(ids("working")).toEqual(["w"]);
    expect(ids("idle")).toEqual(["i"]);
    expect(ids("exited")).toEqual(["e"]);
    expect(ids("all").sort()).toEqual(["i", "n", "w"]);
  });
});

describe("counts", () => {
  it("counts by group", () => {
    const list = [
      withStatus({ kind: "waitingInput" }),
      withStatus({ kind: "error", errorKind: null }),
      withStatus({ kind: "maybeWaiting" }),
      withStatus({ kind: "idle" }),
    ];
    expect(counts(list)).toEqual({ needsYou: 2, working: 1, idle: 1, exited: 0 });
  });
});

describe("groupAgents", () => {
  const list = [
    mkAgent({ id: "1", projectName: "b", entrypoint: "cli" }),
    mkAgent({ id: "2", projectName: "a", entrypoint: "ide" }),
    mkAgent({ id: "3", projectName: "b", entrypoint: "ide" }),
  ];
  it("none gives one unnamed group", () => {
    expect(groupAgents(list, "none")).toEqual([["", list]]);
  });
  it("groups by project in first-seen order", () => {
    const g = groupAgents(list, "project");
    expect(g.map(([k, v]) => [k, v.map((a) => a.id)])).toEqual([
      ["b", ["1", "3"]],
      ["a", ["2"]],
    ]);
  });
  it("groups by entrypoint", () => {
    expect(groupAgents(list, "entrypoint").map(([k]) => k)).toEqual(["cli", "ide"]);
  });
});

describe("format", () => {
  it("contextLevel thresholds", () => {
    expect(contextLevel(160_000, 200_000)).toBe("ok"); // 0.80
    expect(contextLevel(162_000, 200_000)).toBe("warn"); // 0.81
  });
  it("contextLevel high above 0.95 (0.96)", () => {
    expect(contextLevel(192_000, 200_000)).toBe("high");
    expect(contextLevel(100, null)).toBe("ok");
  });
  it("statusLabel", () => {
    expect(statusLabel({ kind: "maybeWaiting" })).toBe("waiting?");
    expect(statusLabel({ kind: "needsPermission", tool: null })).toBe("needs permission");
    expect(statusLabel({ kind: "needsPermission", tool: "Bash" })).toBe(
      "needs permission: Bash",
    );
    expect(statusLabel({ kind: "waitingInput" })).toBe("waiting for you");
    expect(statusLabel({ kind: "error", errorKind: "x" })).toBe("error");
    expect(statusLabel({ kind: "unknown", reason: "r" })).toBe("unknown");
  });
  it("fmtTokens and fmtDuration", () => {
    expect(fmtTokens(182_000)).toBe("182k");
    expect(fmtTokens(1_200_000)).toBe("1.2M");
    expect(fmtTokens(950)).toBe("950");
    expect(fmtDuration(12_000)).toBe("12s");
    expect(fmtDuration(180_000)).toBe("3m");
    expect(fmtDuration(7_500_000)).toBe("2h 5m");
    expect(fmtDuration(-5)).toBe("0s");
  });
});
