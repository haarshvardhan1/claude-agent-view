import { create } from "zustand";
import { call } from "./lib/ipc";
import type { Agent, Change, Filter, GroupBy, Selection } from "./types";

interface State {
  agents: Record<string, Agent>;
  selected: Selection;
  filter: Filter;
  groupBy: GroupBy;
  showExited: boolean;
  search: string;
  setAll: (agents: Agent[]) => void;
  apply: (changes: Change[]) => void;
  select: (sel: Selection) => void;
  setFilter: (f: Filter) => void;
  setGroupBy: (g: GroupBy) => void;
  setShowExited: (v: boolean) => void;
  setSearch: (s: string) => void;
}

export const useStore = create<State>((set) => ({
  agents: {},
  selected: null,
  filter: "all",
  groupBy: "none",
  showExited: false,
  search: "",
  setAll: (agents) =>
    set((s) => {
      const map = Object.fromEntries(agents.map((a) => [a.id, a]));
      const keep = s.selected && map[s.selected.agentId];
      return { agents: map, selected: keep ? s.selected : null };
    }),
  apply: (changes) =>
    set((s) => {
      const agents = { ...s.agents };
      let selected = s.selected;
      for (const c of changes) {
        if (c.type === "agentRemoved") {
          delete agents[c.id];
          if (selected?.agentId === c.id) selected = null;
        } else {
          agents[c.agent.id] = c.agent;
        }
      }
      return { agents, selected };
    }),
  select: (sel) => {
    set({ selected: sel });
    void Promise.resolve(call("set_selected_agent", { id: sel?.agentId ?? null })).catch(() => {});
  },
  setFilter: (filter) => set({ filter }),
  setGroupBy: (groupBy) => set({ groupBy }),
  setShowExited: (showExited) => set({ showExited }),
  setSearch: (search) => set({ search }),
}));
