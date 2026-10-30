import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({ call: vi.fn(() => Promise.resolve()), on: vi.fn() }));

import { AgentCard } from "../components/AgentCard";
import { useStore } from "../store";
import type { Status } from "../types";
import { NOW, ago, mkAgent, withStatus } from "./fixtures";

beforeEach(() => useStore.setState({ agents: {}, selected: null }));

const cases: [Status, string][] = [
  [{ kind: "exited" }, "exited"],
  [{ kind: "needsPermission", tool: null }, "needs permission"],
  [{ kind: "error", errorKind: "rate" }, "error"],
  [{ kind: "waitingInput" }, "waiting for you"],
  [{ kind: "maybeWaiting" }, "waiting?"],
  [{ kind: "working" }, "working"],
  [{ kind: "idle" }, "idle"],
  [{ kind: "unknown", reason: "r" }, "unknown"],
];

describe("AgentCard", () => {
  it.each(cases)("renders status %j as %s", (status, label) => {
    render(<AgentCard agent={withStatus(status)} now={NOW} />);
    expect(screen.getByTestId("status-label")).toHaveTextContent(label);
  });

  it("shows title, project, branch, entrypoint and time in status", () => {
    render(<AgentCard agent={mkAgent()} now={NOW} />);
    expect(screen.getByText("Fix bug")).toBeInTheDocument();
    expect(screen.getByText(/proj · main/)).toBeInTheDocument();
    expect(screen.getByText("cli")).toBeInTheDocument();
    expect(screen.getByText("working 3m")).toBeInTheDocument();
  });

  it("shows pending tool", () => {
    render(
      <AgentCard
        agent={mkAgent({ pendingTool: { name: "Bash", since: ago(12_000) } })}
        now={NOW}
      />,
    );
    expect(screen.getByText("Bash · 12s")).toBeInTheDocument();
  });

  it("shows context with limit, without limit, and hides when used is null", () => {
    const { rerender } = render(
      <AgentCard agent={mkAgent({ context: { used: 182_000, limit: 200_000 } })} now={NOW} />,
    );
    expect(screen.getByText("182k / 200k")).toBeInTheDocument();
    rerender(
      <AgentCard agent={mkAgent({ context: { used: 182_000, limit: null } })} now={NOW} />,
    );
    expect(screen.getByText("182k")).toBeInTheDocument();
    expect(screen.queryByText(/\//)).toBeNull();
    rerender(<AgentCard agent={mkAgent()} now={NOW} />);
    expect(screen.queryByTestId("context")).toBeNull();
  });

  it("fades maybeWaiting", () => {
    render(<AgentCard agent={withStatus({ kind: "maybeWaiting" })} now={NOW} />);
    expect(screen.getByTestId("status-label").className).toContain("opacity-50");
  });

  it("shows running subagent count and expands the tree", () => {
    const sub = (id: string, status: "running" | "done") => ({
      id,
      agentType: "Explore",
      description: `desc ${id}`,
      background: false,
      status,
      lastActivity: ago(5000),
      tokens: { input: 0, output: 0, cacheWrite: 0, cacheRead: 0 },
      pendingTool: null,
      transcriptPath: "/x",
    });
    const agent = mkAgent({ subagents: [sub("s1", "running"), sub("s2", "running"), sub("s3", "done")] });
    useStore.setState({ agents: { [agent.id]: agent } });
    render(<AgentCard agent={agent} now={NOW} />);
    expect(screen.getByText("2 subagents")).toBeInTheDocument();
    expect(screen.queryByText("desc s1")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /expand/i }));
    expect(screen.getByText("desc s1")).toBeInTheDocument();
    expect(screen.getAllByText("5s").length).toBe(3);
    fireEvent.click(screen.getByText("desc s2"));
    expect(useStore.getState().selected).toEqual({ agentId: "a1", subagentId: "s2" });
  });

  it("selects on click", () => {
    render(<AgentCard agent={mkAgent()} now={NOW} />);
    fireEvent.click(screen.getByText("Fix bug"));
    expect(useStore.getState().selected).toEqual({ agentId: "a1" });
  });
});
