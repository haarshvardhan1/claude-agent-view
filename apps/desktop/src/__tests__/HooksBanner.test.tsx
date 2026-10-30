import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({ call: vi.fn(), on: vi.fn() }));

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
    expect(call).toHaveBeenCalledWith("set_settings", {
      settings: { ...base, hooksBannerDismissed: true },
    });
  });
});
