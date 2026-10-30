import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

/** Thin wrapper so tests can `vi.mock("../lib/ipc")`. */
export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

/** Subscribe to a Tauri event; resolves to an unlisten function. */
export function on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  return listen<T>(event, (e) => handler(e.payload));
}

/** Reveal a folder in Finder (wrapped so tests can mock it). */
export function revealInFinder(path: string): Promise<void> {
  return revealItemInDir(path);
}
