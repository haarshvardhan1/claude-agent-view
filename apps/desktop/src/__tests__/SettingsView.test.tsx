import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc", () => ({ call: vi.fn(), on: vi.fn(), openExternal: vi.fn() }));

import { SettingsView } from "../components/SettingsView";
import { call } from "../lib/ipc";
import { useStore } from "../store";

const settings = {
  notify: {
    needsPermission: true,
    waitingInput: true,
    finished: true,
    error: true,
    subagentFinished: false,
    finishedAfterSecs: 30,
  },
  launchAtLogin: false,
  claudeDir: null,
  hooksBannerDismissed: false,
};

function setup(hooks: string, overrides: Record<string, unknown> = {}) {
  const m = vi.mocked(call);
  m.mockReset();
  m.mockImplementation((cmd: string) => {
    if (cmd in overrides) {
      const v = overrides[cmd];
      return v instanceof Error ? Promise.reject(v) : Promise.resolve(v);
    }
    const table: Record<string, unknown> = {
      get_settings: settings,
      hooks_status: hooks,
      notification_permission: "granted",
      watcher_mode: "watching",
      set_settings: undefined,
      install_hooks: { backup: null, changed: true },
      uninstall_hooks: { backup: null, changed: true },
    };
    return Promise.resolve(table[cmd]) as never;
  });
}

beforeEach(() => useStore.setState({ toasts: [] }));

describe("SettingsView", () => {
  it("toggling Finished saves notify.finished=false", async () => {
    setup("installed");
    render(<SettingsView />);
    fireEvent.click(await screen.findByLabelText("Finished"));
    expect(call).toHaveBeenCalledWith("set_settings", {
      settings: { ...settings, notify: { ...settings.notify, finished: false } },
    });
  });

  it("invalidSettings rejection renders a toast", async () => {
    setup("notInstalled", {
      install_hooks: Object.assign(new Error("x"), { code: "invalidSettings", message: "line 3" }),
    });
    render(<SettingsView />);
    fireEvent.click(await screen.findByText("Install"));
    expect(
      await screen.findByText("Your ~/.claude/settings.json isn't valid JSON: line 3"),
    ).toBeInTheDocument();
  });

  it("sidecar_missing_toast", async () => {
    setup("notInstalled", { install_hooks: Object.assign(new Error("x"), { code: "sidecarMissing", message: "m" }) });
    render(<SettingsView />);
    fireEvent.click(await screen.findByText("Install"));
    expect(
      await screen.findByText("The bundled av helper is missing — rebuild the app."),
    ).toBeInTheDocument();
  });

  it("repair_shown_when_broken", async () => {
    setup("broken");
    render(<SettingsView />);
    expect(await screen.findByText("Repair")).toBeInTheDocument();
    expect(screen.getByText("Uninstall")).toBeInTheDocument();
    expect(screen.queryByText("Install")).toBeNull();
    fireEvent.click(screen.getByText("Repair"));
    await waitFor(() => expect(call).toHaveBeenCalledWith("install_hooks"));
  });

  it("finished_secs_ignores_nan", async () => {
    setup("installed");
    render(<SettingsView />);
    const input = await screen.findByLabelText("Finished after N seconds of work");
    await act(async () => {
      fireEvent.change(input, { target: { value: "" } });
    });
    expect(vi.mocked(call).mock.calls.filter((c) => c[0] === "set_settings")).toHaveLength(0);
    fireEvent.change(input, { target: { value: "12" } });
    expect(call).toHaveBeenCalledWith("set_settings", {
      settings: { ...settings, notify: { ...settings.notify, finishedAfterSecs: 12 } },
    });
  });
});
