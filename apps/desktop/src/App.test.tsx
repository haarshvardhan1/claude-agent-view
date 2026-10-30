import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AgentCount } from "./App";

describe("AgentCount", () => {
  it("shows the agent count", () => {
    render(<AgentCount count={2} />);
    expect(screen.getByText("2 agents")).toBeInTheDocument();
  });
});
