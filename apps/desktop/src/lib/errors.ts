import { useStore } from "../store";

/** Turn a rejected command (`{code, message}`) into a user-facing toast. */
export function toastError(e: unknown): void {
  const err = (e ?? {}) as { code?: string; message?: string };
  const msg = err.message ?? String(e);
  let text = msg;
  if (err.code === "invalidSettings") text = `Your ~/.claude/settings.json isn't valid JSON: ${msg}`;
  else if (err.code === "sidecarMissing") text = "The bundled av helper is missing — rebuild the app.";
  useStore.getState().pushToast(text);
}
