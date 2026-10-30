import { useEffect, useState } from "react";
import { call } from "../lib/ipc";
import { toastError } from "../lib/errors";
import type { AppSettings, HookReport, HooksStatus } from "../types";

export function HooksBanner() {
  const [show, setShow] = useState(false);
  const [settings, setSettings] = useState<AppSettings | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([call<HooksStatus>("hooks_status"), call<AppSettings>("get_settings")])
      .then(([status, s]) => {
        if (cancelled) return;
        setSettings(s);
        setShow(status !== "installed" && !s.hooksBannerDismissed);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  if (!show || !settings) return null;

  const install = () =>
    call<HookReport>("install_hooks")
      .then(() => setShow(false))
      .catch(toastError);
  const dismiss = () => {
    setShow(false);
    call("set_settings", { settings: { ...settings, hooksBannerDismissed: true } }).catch(toastError);
  };

  return (
    <div className="flex items-center gap-3 border-b border-amber-300 bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-100">
      <span className="flex-1">Install hooks to get reliable permission alerts</span>
      <button type="button" onClick={install} className="rounded bg-amber-600 px-2 py-1 text-white">
        Install
      </button>
      <button type="button" onClick={dismiss} className="px-2 py-1 underline">
        Dismiss
      </button>
    </div>
  );
}
