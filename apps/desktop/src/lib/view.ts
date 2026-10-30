import type { Agent, Filter, GroupBy, Status, StatusGroup } from "../types";

export interface ViewState {
  agents: Record<string, Agent>;
  filter: Filter;
  showExited: boolean;
  search: string;
}

const ORDER: StatusGroup[] = ["needsYou", "working", "idle", "exited"];
const DAY_MS = 24 * 3600 * 1000;

export function statusGroup(s: Status): StatusGroup {
  switch (s.kind) {
    case "needsPermission":
    case "error":
    case "waitingInput":
      return "needsYou";
    case "working":
    case "maybeWaiting":
      return "working";
    case "exited":
      return "exited";
    default:
      return "idle";
  }
}

const ts = (iso: string | null) => (iso ? Date.parse(iso) : 0);

/** Agents after search and the exited rule, before the chip filter. */
export function baseVisible(state: ViewState, now: Date): Agent[] {
  const q = state.search.trim().toLowerCase();
  const showExited = state.showExited || state.filter === "exited";
  return Object.values(state.agents).filter((a) => {
    if (q) {
      const hay = [a.title, a.projectName, a.gitBranch];
      if (!hay.some((h) => h != null && h.toLowerCase().includes(q))) return false;
    }
    if (a.status.kind === "exited") {
      return showExited && now.getTime() - ts(a.statusSince) <= DAY_MS;
    }
    return true;
  });
}

export function visibleAgents(state: ViewState, now: Date): Agent[] {
  const base = baseVisible(state, now);
  if (state.filter === "all") return base;
  return base.filter((a) => statusGroup(a.status) === state.filter);
}

export function sortAgents(agents: Agent[]): Agent[] {
  return [...agents].sort(
    (a, b) =>
      ORDER.indexOf(statusGroup(a.status)) - ORDER.indexOf(statusGroup(b.status)) ||
      ts(b.lastActivity) - ts(a.lastActivity),
  );
}

export function groupAgents(agents: Agent[], by: GroupBy): [string, Agent[]][] {
  if (by === "none") return [["", agents]];
  const map = new Map<string, Agent[]>();
  for (const a of agents) {
    const k = by === "project" ? a.projectName : a.entrypoint;
    const list = map.get(k);
    if (list) list.push(a);
    else map.set(k, [a]);
  }
  return [...map.entries()];
}

export function counts(agents: Agent[]): Record<StatusGroup, number> {
  const c: Record<StatusGroup, number> = { needsYou: 0, working: 0, idle: 0, exited: 0 };
  for (const a of agents) c[statusGroup(a.status)]++;
  return c;
}
