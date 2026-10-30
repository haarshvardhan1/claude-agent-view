import { useEffect, useRef, useState } from "react";
import { call, onWindowFocus } from "../lib/ipc";
import { toastError } from "../lib/errors";
import type { AppSettings, HookReport, HooksStatus } from "../types";

export function HooksBanner() {
  const [status, setStatus] = useState<HooksStatus | null>(null);
  const [dismissed, setDismissed] = useState(true);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    const refresh = () =>
      Promise.all([call<HooksStatus>("hooks_status"), call<AppSettings>("get_settings")])
        .then(([st, s]) => {
          if (cancelled) return;
          setStatus(st);
          setDismissed(s.hooksBannerDismissed);
        })
        .catch(() => {});
    void refresh();
    const off = onWindowFocus(() => void refresh());
    return () => {
      cancelled = true;
      off();
    };
  }, []);

  if (status === null || status === "installed" || dismissed) return null;

  const install = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await call<HookReport>("install_hooks");
      setStatus("installed");
    } catch (e) {
      toastError(e);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  const dismiss = async () => {
    setDismissed(true);
    try {
      const fresh = await call<AppSettings>("get_settings");
      await call("set_settings", { settings: { ...fresh, hooksBannerDismissed: true } });
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <div className="flex items-center gap-3 border-b border-amber-300 bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-100">
      <span className="flex-1">Install hooks to get reliable permission alerts</span>
      <button
        type="button"
        disabled={busy}
        onClick={install}
        className="rounded bg-amber-600 px-2 py-1 text-white disabled:opacity-50"
      >
        {status === "broken" ? "Repair" : "Install"}
      </button>
      <button type="button" onClick={dismiss} className="px-2 py-1 underline">
        Dismiss
      </button>
    </div>
  );
}
