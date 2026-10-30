import { useCallback, useEffect, useState } from "react";
import { toastError } from "../lib/errors";
import { call, openExternal } from "../lib/ipc";
import type { AppSettings, HookReport, HooksStatus, NotifySettings } from "../types";
import { Toasts } from "./Toasts";

const NOTIFY_ROWS: [keyof Omit<NotifySettings, "finishedAfterSecs">, string][] = [
  ["needsPermission", "Needs permission"],
  ["waitingInput", "Waiting for input"],
  ["finished", "Finished"],
  ["error", "Error"],
  ["subagentFinished", "Subagent finished"],
];

const STATUS_TEXT: Record<HooksStatus, string> = {
  installed: "Hooks installed",
  notInstalled: "Hooks not installed",
  broken: "Hooks need repair",
};

const NOTIF_URL = "x-apple.systempreferences:com.apple.preference.notifications";

export function SettingsView() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [hooks, setHooks] = useState<HooksStatus | null>(null);
  const [backup, setBackup] = useState<string | null>(null);
  const [perm, setPerm] = useState<string>("unknown");
  const [mode, setMode] = useState<string>("watching");
  const [dir, setDir] = useState("");

  useEffect(() => {
    call<AppSettings>("get_settings")
      .then((s) => {
        setSettings(s);
        setDir(s.claudeDir ?? "");
      })
      .catch(toastError);
    call<HooksStatus>("hooks_status").then(setHooks).catch(toastError);
    call<string>("notification_permission").then(setPerm).catch(() => {});
    call<string>("watcher_mode").then(setMode).catch(() => {});
  }, []);

  const save = useCallback((next: AppSettings) => {
    setSettings(next);
    call("set_settings", { settings: next }).catch(toastError);
  }, []);

  const hookAction = (cmd: "install_hooks" | "uninstall_hooks") =>
    call<HookReport>(cmd)
      .then((r) => setBackup(r.backup))
      .catch(toastError)
      .finally(() => {
        call<HooksStatus>("hooks_status").then(setHooks).catch(toastError);
      });

  const btn = "rounded border border-gray-300 px-2 py-1 text-sm dark:border-gray-700";

  return (
    <div className="min-h-screen bg-white p-6 text-sm text-gray-900 dark:bg-gray-950 dark:text-gray-100">
      <h1 className="mb-4 text-xl font-semibold">Settings</h1>
      {!settings ? null : (
        <div className="flex flex-col gap-6">
          <section>
            <h2 className="mb-2 font-medium">Hooks</h2>
            <p className="mb-2">{hooks ? STATUS_TEXT[hooks] : "Checking…"}</p>
            <div className="flex gap-2">
              {hooks === "notInstalled" && (
                <button type="button" className={btn} onClick={() => hookAction("install_hooks")}>
                  Install
                </button>
              )}
              {hooks === "broken" && (
                <button type="button" className={btn} onClick={() => hookAction("install_hooks")}>
                  Repair
                </button>
              )}
              {(hooks === "installed" || hooks === "broken") && (
                <button type="button" className={btn} onClick={() => hookAction("uninstall_hooks")}>
                  Uninstall
                </button>
              )}
            </div>
            {backup && <p className="mt-2 text-xs text-gray-500">Backup: {backup}</p>}
          </section>

          <section>
            <h2 className="mb-2 font-medium">Notifications</h2>
            {perm === "denied" && (
              <p className="mb-2 text-red-600 dark:text-red-400">
                Notifications are denied for Agent Viewer.
              </p>
            )}
            <div className="flex flex-col gap-1">
              {NOTIFY_ROWS.map(([key, label]) => (
                <label key={key} className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={settings.notify[key]}
                    onChange={(e) =>
                      save({ ...settings, notify: { ...settings.notify, [key]: e.target.checked } })
                    }
                  />
                  {label}
                </label>
              ))}
              <label className="mt-1 flex items-center gap-2">
                Finished after
                <input
                  type="number"
                  min={0}
                  step={1}
                  aria-label="Finished after N seconds of work"
                  className="w-20 rounded border border-gray-300 bg-transparent px-1 dark:border-gray-700"
                  value={settings.notify.finishedAfterSecs}
                  onChange={(e) => {
                    const n = e.target.valueAsNumber;
                    if (Number.isNaN(n)) return;
                    save({
                      ...settings,
                      notify: { ...settings.notify, finishedAfterSecs: Math.max(0, Math.trunc(n)) },
                    });
                  }}
                />
                seconds of work
              </label>
            </div>
            <p className="mt-2 text-xs text-gray-500">
              If notifications don't appear, allow Agent Viewer in{" "}
              <a
                href={NOTIF_URL}
                className="underline"
                onClick={(e) => {
                  e.preventDefault();
                  openExternal(NOTIF_URL).catch(toastError);
                }}
              >
                System Settings → Notifications
              </a>
            </p>
          </section>

          <section>
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={settings.launchAtLogin}
                onChange={(e) => save({ ...settings, launchAtLogin: e.target.checked })}
              />
              Launch at login
            </label>
          </section>

          <section>
            <label className="flex flex-col gap-1">
              <span className="font-medium">Claude folder</span>
              <input
                type="text"
                placeholder="~/.claude"
                className="rounded border border-gray-300 bg-transparent px-2 py-1 dark:border-gray-700"
                value={dir}
                onChange={(e) => setDir(e.target.value)}
                onBlur={() => {
                  const v = dir.trim() === "" ? null : dir.trim();
                  if (v !== settings.claudeDir) save({ ...settings, claudeDir: v });
                }}
              />
            </label>
            <p className="mt-1 text-xs text-gray-500">Takes effect after restarting Agent Viewer</p>
          </section>

          {mode === "polling" && (
            <p className="text-xs text-gray-500">Polling mode: file watching is unavailable, so updates may lag.</p>
          )}
        </div>
      )}
      <Toasts />
    </div>
  );
}
