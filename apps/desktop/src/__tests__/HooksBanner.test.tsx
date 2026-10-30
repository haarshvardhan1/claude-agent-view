import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({
  call: vi.fn(),
  on: vi.fn(),
  onWindowFocus: (h: () => void) => {
    window.addEventListener("focus", h);
    return () => window.removeEventListener("focus", h);
  },
}));

import { HooksBanner } from "../components/HooksBanner";
import { call } from "../lib/ipc";

const base = {
  notify: { needsPermission: true, waitingInput: true, finished: true, error: true, subagentFinished: false, finishedAfterSecs: 30 },
  launchAtLogin: false,
  claudeDir: null,
};

function setup(status: string, dismissed: boolean) {
  vi.mocked(call).mockReset();
  vi.mocked(call).mockImplementation((cmd: string) => {
    if (cmd === "hooks_status") return Promise.resolve(status) as never;
    if (cmd === "get_settings") return Promise.resolve({ ...base, hooksBannerDismissed: dismissed }) as never;
    if (cmd === "install_hooks") return Promise.resolve({ backup: null, changed: true }) as never;
    return Promise.resolve(undefined) as never;
  });
}

const TEXT = "Install hooks to get reliable permission alerts";

describe("HooksBanner", () => {
  beforeEach(() => vi.clearAllMocks());

  it("hidden when installed", async () => {
    setup("installed", false);
    render(<HooksBanner />);
    await waitFor(() => expect(call).toHaveBeenCalledWith("get_settings"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByText(TEXT)).toBeNull();
  });

  it("hidden when dismissed", async () => {
    setup("notInstalled", true);
    render(<HooksBanner />);
    await waitFor(() => expect(call).toHaveBeenCalledWith("get_settings"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByText(TEXT)).toBeNull();
  });

  it("Install calls install_hooks and hides", async () => {
    setup("notInstalled", false);
    render(<HooksBanner />);
    fireEvent.click(await screen.findByText("Install"));
    await waitFor(() => expect(call).toHaveBeenCalledWith("install_hooks"));
    await waitFor(() => expect(screen.queryByText(TEXT)).toBeNull());
  });

  it("Dismiss saves hooksBannerDismissed", async () => {
    setup("broken", false);
    render(<HooksBanner />);
    fireEvent.click(await screen.findByText("Dismiss"));
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith("set_settings", {
        settings: { ...base, hooksBannerDismissed: true },
      }),
    );
  });

  it("double click on Install calls install_hooks once", async () => {
    setup("notInstalled", false);
    let release: (v: unknown) => void = () => {};
    const inner = vi.mocked(call).getMockImplementation()!;
    vi.mocked(call).mockImplementation((cmd: string, a?: Record<string, unknown>) =>
      cmd === "install_hooks" ? (new Promise((r) => (release = r)) as never) : inner(cmd, a),
    );
    render(<HooksBanner />);
    const b = await screen.findByText("Install");
    fireEvent.click(b);
    fireEvent.click(b);
    release({ backup: null, changed: true });
    await waitFor(() => expect(screen.queryByText(TEXT)).toBeNull());
    expect(vi.mocked(call).mock.calls.filter((c) => c[0] === "install_hooks")).toHaveLength(1);
  });

  it("Dismiss re-reads settings so later changes are kept", async () => {
    setup("notInstalled", false);
    render(<HooksBanner />);
    const b = await screen.findByText("Dismiss");
    const inner = vi.mocked(call).getMockImplementation()!;
    vi.mocked(call).mockImplementation((cmd: string, a?: Record<string, unknown>) =>
      cmd === "get_settings"
        ? (Promise.resolve({ ...base, launchAtLogin: true, hooksBannerDismissed: false }) as never)
        : inner(cmd, a),
    );
    fireEvent.click(b);
    await waitFor(() =>
      expect(call).toHaveBeenCalledWith("set_settings", {
        settings: { ...base, launchAtLogin: true, hooksBannerDismissed: true },
      }),
    );
  });

  it("shows Repair when broken and re-syncs on focus", async () => {
    setup("broken", false);
    render(<HooksBanner />);
    expect(await screen.findByText("Repair")).toBeInTheDocument();
    const inner = vi.mocked(call).getMockImplementation()!;
    vi.mocked(call).mockImplementation((cmd: string, a?: Record<string, unknown>) =>
      cmd === "hooks_status" ? (Promise.resolve("installed") as never) : inner(cmd, a),
    );
    window.dispatchEvent(new Event("focus"));
    await waitFor(() => expect(screen.queryByText(TEXT)).toBeNull());
  });
});
