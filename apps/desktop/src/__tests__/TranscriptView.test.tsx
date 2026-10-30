import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({
  call: vi.fn(),
  on: vi.fn(),
  revealInFinder: vi.fn(() => Promise.resolve()),
}));

import { DetailPanel } from "../components/DetailPanel";
import { call, revealInFinder } from "../lib/ipc";
import { useStore } from "../store";
import type { TranscriptEntry, TranscriptPage } from "../types";
import { mkAgent } from "./fixtures";

const mockCall = vi.mocked(call);
type Req = { kind: "tail" } | { kind: "before" | "after"; offset: number };
type Handler = (agentId: string, req: Req) => Promise<TranscriptPage> | TranscriptPage;

function setup(handler: Handler) {
  mockCall.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
    if (cmd !== "get_transcript") return Promise.resolve();
    return Promise.resolve(handler(args!.agentId as string, args!.req as Req));
  }) as typeof call);
}
const user = (text: string): TranscriptEntry => ({ kind: "userPrompt", text, timestamp: null });
const asst = (text: string): TranscriptEntry => ({ kind: "assistantText", text, timestamp: null });
const tool = (id: string, over: Partial<Extract<TranscriptEntry, { kind: "toolCall" }>> = {}): TranscriptEntry => ({
  kind: "toolCall", id, name: "Bash", summary: "ls -la", input: '{"command":"ls"}',
  result: null, isError: false, timestamp: null, ...over,
});
const page = (entries: TranscriptEntry[], startOffset: number, endOffset: number): TranscriptPage => ({
  entries, startOffset, endOffset,
});
const flush = () => act(async () => { await Promise.resolve(); });

beforeEach(() => {
  vi.clearAllMocks();
  const a = mkAgent({ id: "a1" });
  useStore.setState({ agents: { a1: a }, selected: { agentId: "a1" } });
});

const bump = () =>
  act(() => {
    useStore.getState().apply([{ type: "agentUpdated", agent: mkAgent({ id: "a1", title: "changed" }) }]);
  });
const afterCalls = () =>
  mockCall.mock.calls.filter((c) => (c[1] as { req: Req }).req.kind === "after").map((c) => (c[1] as { req: Req }).req);

describe("TranscriptView", () => {
  it("renders user and assistant messages preserving text", async () => {
    setup(() => page([user("hello\n  world"), asst("hi there")], 0, 10));
    render(<DetailPanel />);
    const u = await screen.findByText(/hello/);
    expect(u.textContent).toBe("hello\n  world");
    expect(u.className).toContain("whitespace-pre-wrap");
    expect(screen.getByText("hi there")).toBeInTheDocument();
    expect(mockCall).toHaveBeenCalledWith("get_transcript", { agentId: "a1", subagentId: null, req: { kind: "tail" } });
  });

  it("tool call is collapsed, then expands to input and result; errors are red", async () => {
    setup(() => page([tool("t1", { result: "boom", isError: true }), tool("t2")], 0, 10));
    render(<DetailPanel />);
    expect((await screen.findAllByText("Bash: ls -la"))).toHaveLength(2);
    expect(screen.queryByText("boom")).toBeNull();
    expect(screen.getAllByText("running…")).toHaveLength(1);
    fireEvent.click(screen.getAllByText("Bash: ls -la")[0]);
    expect(screen.getByText('{"command":"ls"}')).toBeInTheDocument();
    const res = screen.getByText("boom");
    expect(res.tagName).toBe("PRE");
    expect(res.className).toContain("text-red");
  });

  it("thinking is collapsed by default", async () => {
    setup(() => page([{ kind: "thinking", text: "secret musings", timestamp: null }], 0, 5));
    render(<DetailPanel />);
    await screen.findByText(/Thinking/);
    expect(screen.queryByText("secret musings")).toBeNull();
    fireEvent.click(screen.getByText(/Thinking/));
    expect(screen.getByText("secret musings")).toBeInTheDocument();
  });

  it("Load earlier calls before with startOffset, prepends, and hides at offset 0", async () => {
    setup((_, req) =>
      req.kind === "tail" ? page([asst("new")], 100, 200) : page([asst("old")], 0, 100),
    );
    render(<DetailPanel />);
    await screen.findByText("new");
    fireEvent.click(screen.getByText("Load earlier"));
    await screen.findByText("old");
    expect(mockCall).toHaveBeenCalledWith("get_transcript", { agentId: "a1", subagentId: null, req: { kind: "before", offset: 100 } });
    const texts = screen.getAllByText(/^(old|new)$/).map((n) => n.textContent);
    expect(texts).toEqual(["old", "new"]);
    expect(screen.queryByText("Load earlier")).toBeNull();
  });

  it("hides Load earlier when a before page is empty", async () => {
    setup((_, req) => (req.kind === "tail" ? page([asst("x")], 50, 60) : page([], 50, 50)));
    render(<DetailPanel />);
    await screen.findByText("x");
    fireEvent.click(screen.getByText("Load earlier"));
    await waitFor(() => expect(screen.queryByText("Load earlier")).toBeNull());
  });

  it("live_append_after_change", async () => {
    setup((_, req) => (req.kind === "tail" ? page([asst("one")], 0, 10) : page([asst("two")], 10, 20)));
    render(<DetailPanel />);
    await screen.findByText("one");
    bump();
    await screen.findByText("two");
    expect(afterCalls()).toEqual([{ kind: "after", offset: 10 }]);
  });

  it("loops while pages are full and runs one fetch at a time (dirty refetch)", async () => {
    const full = Array.from({ length: 200 }, (_, i) => asst(`m${i}`));
    let release: (p: TranscriptPage) => void = () => {};
    const gate = new Promise<TranscriptPage>((r) => (release = r));
    let n = 0;
    setup((_, req) => {
      if (req.kind === "tail") return page([asst("one")], 0, 10);
      n++;
      if (n === 1) return gate;
      if (n === 2) return page([asst("tail")], 210, 215);
      return page([], 215, 215);
    });
    render(<DetailPanel />);
    await screen.findByText("one");
    bump();
    await flush();
    bump(); // mid-fetch: must not start a second fetch
    await flush();
    expect(afterCalls()).toHaveLength(1);
    await act(async () => release(page(full, 10, 210)));
    await screen.findByText("tail");
    expect(afterCalls().map((r) => (r as { offset: number }).offset)).toEqual([10, 210, 215]);
  });

  it("orphan_result_patches_call", async () => {
    setup((_, req) =>
      req.kind === "tail"
        ? page([tool("t1")], 0, 10)
        : page([{ kind: "toolResult", toolUseId: "t1", result: "done!", isError: true, timestamp: null }], 10, 20),
    );
    render(<DetailPanel />);
    await screen.findByText("running…");
    bump();
    await waitFor(() => expect(screen.queryByText("running…")).toBeNull());
    expect(screen.getAllByText(/Bash: ls -la/)).toHaveLength(1);
    fireEvent.click(screen.getByText("Bash: ls -la"));
    expect(screen.getByText("done!").className).toContain("text-red");
  });

  it("applies a pending result when Load earlier brings in the call", async () => {
    setup((_, req) => {
      if (req.kind === "tail") return page([asst("late")], 100, 200);
      if (req.kind === "after")
        return page([{ kind: "toolResult", toolUseId: "t9", result: "R9", isError: false, timestamp: null }], 200, 210);
      return page([tool("t9")], 0, 100);
    });
    render(<DetailPanel />);
    await screen.findByText("late");
    bump();
    await waitFor(() => expect(afterCalls()).toHaveLength(1));
    await flush();
    fireEvent.click(screen.getByText("Load earlier"));
    await screen.findByText("Bash: ls -la");
    expect(screen.queryByText("running…")).toBeNull();
    fireEvent.click(screen.getByText("Bash: ls -la"));
    expect(screen.getByText("R9")).toBeInTheDocument();
  });

  it("truncation_replaces_view", async () => {
    setup((_, req) => (req.kind === "tail" ? page([asst("old a"), asst("old b")], 0, 500) : page([asst("fresh")], 0, 40)));
    render(<DetailPanel />);
    await screen.findByText("old a");
    bump();
    await screen.findByText("fresh");
    expect(screen.queryByText("old a")).toBeNull();
    expect(screen.queryByText("old b")).toBeNull();
  });

  it("stale_response_ignored", async () => {
    useStore.setState({
      agents: { a1: mkAgent({ id: "a1" }), a2: mkAgent({ id: "a2", title: "Two" }) },
    });
    let releaseA1: (p: TranscriptPage) => void = () => {};
    setup((id) =>
      id === "a1" ? new Promise<TranscriptPage>((r) => (releaseA1 = r)) : page([asst("from a2")], 0, 5),
    );
    render(<DetailPanel />);
    await flush();
    act(() => useStore.setState({ selected: { agentId: "a2" } }));
    await screen.findByText("from a2");
    await act(async () => releaseA1(page([asst("from a1")], 0, 5)));
    expect(screen.queryByText("from a1")).toBeNull();
    expect(screen.getByText("from a2")).toBeInTheDocument();
  });

  it("shows 'no longer available' on notFound for an exited agent", async () => {
    useStore.setState({ agents: { a1: mkAgent({ id: "a1", status: { kind: "exited" } }) } });
    mockCall.mockImplementation((() => Promise.reject({ code: "notFound", message: "gone" })) as typeof call);
    render(<DetailPanel />);
    expect(await screen.findByText("This session is no longer available")).toBeInTheDocument();
  });

  it("shows 'No transcript yet' on notFound for a live agent without a transcript", async () => {
    mockCall.mockImplementation((() => Promise.reject({ code: "notFound", message: "gone" })) as typeof call);
    render(<DetailPanel />);
    expect(await screen.findByText("No transcript yet")).toBeInTheDocument();
    expect(screen.queryByText("This session is no longer available")).toBeNull();
  });

  it("refetches the tail after notFound when the agent updates with a transcriptPath", async () => {
    let exists = false;
    mockCall.mockImplementation(((cmd: string, args?: Record<string, unknown>) => {
      if (cmd !== "get_transcript") return Promise.resolve();
      if (!exists) return Promise.reject({ code: "notFound", message: "no transcript" });
      const req = args!.req as Req;
      return Promise.resolve(req.kind === "tail" ? page([user("first prompt"), asst("reply")], 0, 30) : page([], 30, 30));
    }) as typeof call);
    render(<DetailPanel />);
    expect(await screen.findByText("No transcript yet")).toBeInTheDocument();
    exists = true;
    act(() => {
      useStore.getState().apply([
        { type: "agentUpdated", agent: mkAgent({ id: "a1", transcriptPath: "/p/a1.jsonl" }) },
      ]);
    });
    expect(await screen.findByText("first prompt")).toBeInTheDocument();
    expect(screen.getByText("reply")).toBeInTheDocument();
    expect(screen.queryByText("No transcript yet")).toBeNull();
    const tails = mockCall.mock.calls.filter((c) => (c[1] as { req: Req }).req.kind === "tail");
    expect(tails).toHaveLength(2);
    expect(afterCalls()).toEqual([]);
  });

  it("an update while the first tail is in flight does not start a second tail", async () => {
    let release: (p: TranscriptPage) => void = () => {};
    setup((_, req) =>
      req.kind === "tail" ? new Promise<TranscriptPage>((r) => (release = r)) : page([asst("more")], 10, 20),
    );
    render(<DetailPanel />);
    await flush();
    bump();
    await act(async () => release(page([asst("one")], 0, 10)));
    await screen.findByText("more");
    const tails = mockCall.mock.calls.filter((c) => (c[1] as { req: Req }).req.kind === "tail");
    expect(tails).toHaveLength(1);
    expect(afterCalls()).toEqual([{ kind: "after", offset: 10 }]);
  });

  it("header shows details and reveals folder", async () => {
    setup(() => page([], 0, 0));
    useStore.setState({
      agents: { a1: mkAgent({ id: "a1", cwd: "/work/x", pid: 4242, tokens: { input: 1500, output: 20, cacheWrite: 3000, cacheRead: 2_000_000 } }) },
    });
    render(<DetailPanel />);
    expect(screen.getByText("/work/x")).toBeInTheDocument();
    expect(screen.getByText("4242")).toBeInTheDocument();
    expect(screen.getByText(/in 2k \/ out 20 \/ cache write 3k \/ cache read 2M/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("Open folder in Finder"));
    expect(revealInFinder).toHaveBeenCalledWith("/work/x");
    await flush();
  });

  it("load_earlier_double_click_no_duplicates", async () => {
    let release: (p: TranscriptPage) => void = () => {};
    setup((_, req) =>
      req.kind === "tail" ? page([asst("new")], 100, 200) : new Promise<TranscriptPage>((r) => (release = r)),
    );
    render(<DetailPanel />);
    await screen.findByText("new");
    const btn = screen.getByText("Load earlier");
    fireEvent.click(btn);
    fireEvent.click(btn);
    await flush();
    expect(btn).toBeDisabled();
    await act(async () => release(page([asst("old")], 0, 100)));
    await screen.findByText("old");
    const befores = mockCall.mock.calls.filter((c) => (c[1] as { req: Req }).req.kind === "before");
    expect(befores).toHaveLength(1);
    expect(screen.getAllByText("old")).toHaveLength(1);
  });

  it("load_earlier_dropped_after_replace", async () => {
    let release: (p: TranscriptPage) => void = () => {};
    setup((_, req) => {
      if (req.kind === "tail") return page([asst("old tail")], 100, 500);
      if (req.kind === "after") return page([asst("fresh")], 0, 40);
      return new Promise<TranscriptPage>((r) => (release = r));
    });
    render(<DetailPanel />);
    await screen.findByText("old tail");
    fireEvent.click(screen.getByText("Load earlier"));
    await flush();
    bump();
    await screen.findByText("fresh");
    await act(async () => release(page([asst("stale older")], 0, 100)));
    expect(screen.queryByText("stale older")).toBeNull();
    expect(screen.getByText("fresh")).toBeInTheDocument();
  });
});
