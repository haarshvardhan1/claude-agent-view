import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({ call: vi.fn(() => Promise.resolve()), on: vi.fn() }));

import { call } from "../lib/ipc";
import { useStore } from "../store";
import { mkAgent } from "./fixtures";

beforeEach(() => {
  useStore.setState({ agents: {}, selected: null });
  vi.mocked(call).mockClear();
});

describe("store.apply", () => {
  it("adds, updates and removes agents", () => {
    const a = mkAgent({ id: "a", title: "one" });
    useStore.getState().apply([{ type: "agentAdded", agent: a }]);
    expect(useStore.getState().agents.a.title).toBe("one");
    useStore.getState().apply([{ type: "agentUpdated", agent: { ...a, title: "two" } }]);
    expect(useStore.getState().agents.a.title).toBe("two");
    useStore.getState().apply([{ type: "agentRemoved", id: "a" }]);
    expect(useStore.getState().agents.a).toBeUndefined();
  });

  it("clears selection when the selected agent is removed", () => {
    const a = mkAgent({ id: "a" });
    useStore.getState().setAll([a]);
    useStore.getState().select({ agentId: "a" });
    useStore.getState().apply([{ type: "agentRemoved", id: "a" }]);
    expect(useStore.getState().selected).toBeNull();
  });

  it("keeps selection when another agent is removed", () => {
    useStore.getState().setAll([mkAgent({ id: "a" }), mkAgent({ id: "b" })]);
    useStore.getState().select({ agentId: "a" });
    useStore.getState().apply([{ type: "agentRemoved", id: "b" }]);
    expect(useStore.getState().selected).toEqual({ agentId: "a" });
  });
});

describe("store.select", () => {
  it("stores selection and tells the backend", () => {
    useStore.getState().select({ agentId: "a", subagentId: "s" });
    expect(useStore.getState().selected).toEqual({ agentId: "a", subagentId: "s" });
    expect(call).toHaveBeenCalledWith("set_selected_agent", { id: "a" });
  });
});
