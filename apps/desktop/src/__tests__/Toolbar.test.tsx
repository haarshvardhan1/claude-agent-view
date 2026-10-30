import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({ call: vi.fn(() => Promise.resolve()), on: vi.fn() }));

import { AgentList } from "../components/AgentList";
import { Toolbar } from "../components/Toolbar";
import { useStore } from "../store";
import { NOW, ago, mkAgent, withStatus } from "./fixtures";

const exited = (id: string) =>
  withStatus({ kind: "exited" }, { id, title: `gone ${id}`, statusSince: ago(3_600_000) });

beforeEach(() => {
  const list = [exited("e1"), exited("e2"), mkAgent({ id: "live", title: "alive" })];
  useStore.setState({
    agents: Object.fromEntries(list.map((a) => [a.id, a])),
    selected: null,
    filter: "all",
    showExited: false,
    search: "",
    groupBy: "none",
  });
});

describe("Toolbar exited chip", () => {
  it("counts exited agents within 24h even when showExited is off, and clicking lists them", () => {
    render(
      <>
        <Toolbar now={NOW} />
        <AgentList now={NOW} claudeDir="~/.claude" />
      </>,
    );
    const chip = screen.getByRole("button", { name: /^Exited/ });
    expect(chip).toHaveTextContent("Exited 2");
    expect(screen.queryByText("gone e1")).toBeNull();
    fireEvent.click(chip);
    expect(screen.getByText("gone e1")).toBeInTheDocument();
    expect(screen.getByText("gone e2")).toBeInTheDocument();
    expect(screen.queryByText("alive")).toBeNull();
  });

  it("shows the same count when showExited is on", () => {
    useStore.setState({ showExited: true });
    render(<Toolbar now={NOW} />);
    expect(screen.getByRole("button", { name: /^Exited/ })).toHaveTextContent("Exited 2");
  });
});
