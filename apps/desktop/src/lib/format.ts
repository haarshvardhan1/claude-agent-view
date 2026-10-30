import type { Status } from "../types";

export function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${parseFloat((n / 1_000_000).toFixed(1))}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}

export function fmtDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  return rm === 0 ? `${h}h` : `${h}h ${rm}m`;
}

export function contextLevel(used: number, limit: number | null): "ok" | "warn" | "high" {
  if (!limit) return "ok";
  const r = used / limit;
  if (r > 0.95) return "high";
  if (r > 0.8) return "warn";
  return "ok";
}

export function statusLabel(s: Status): string {
  switch (s.kind) {
    case "maybeWaiting":
      return "waiting?";
    case "needsPermission":
      return s.tool ? `needs permission: ${s.tool}` : "needs permission";
    case "waitingInput":
      return "waiting for you";
    case "error":
      return "error";
    case "working":
      return "working";
    case "idle":
      return "idle";
    case "exited":
      return "exited";
    case "unknown":
      return "unknown";
  }
}
