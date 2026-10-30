import { baseVisible, counts } from "../lib/view";
import { useStore } from "../store";
import type { Filter, GroupBy, StatusGroup } from "../types";

const CHIPS: [Filter, string][] = [
  ["all", "All"],
  ["needsYou", "Needs you"],
  ["working", "Working"],
  ["idle", "Idle"],
  ["exited", "Exited"],
];

export function Toolbar({ now }: { now: Date }) {
  const agents = useStore((s) => s.agents);
  const filter = useStore((s) => s.filter);
  const showExited = useStore((s) => s.showExited);
  const search = useStore((s) => s.search);
  const groupBy = useStore((s) => s.groupBy);
  const { setFilter, setGroupBy, setShowExited, setSearch } = useStore.getState();
  const base = baseVisible({ agents, filter, showExited, search }, now);
  const c = counts(base);

  return (
    <div className="flex flex-wrap items-center gap-3 border-b border-gray-200 px-3 py-2 dark:border-gray-800">
      <input
        type="search"
        aria-label="Search"
        placeholder="Search title, project, branch"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        className="w-56 rounded border border-gray-300 bg-transparent px-2 py-1 text-sm dark:border-gray-700"
      />
      <div className="flex gap-1">
        {CHIPS.map(([f, label]) => (
          <button
            key={f}
            type="button"
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
            className={`rounded-full px-2.5 py-0.5 text-xs ${filter === f ? "bg-blue-600 text-white" : "bg-gray-200 dark:bg-gray-800"}`}
          >
            {label}
            {f !== "all" && ` ${c[f as StatusGroup]}`}
          </button>
        ))}
      </div>
      <label className="flex items-center gap-1 text-xs">
        Group by
        <select
          value={groupBy}
          onChange={(e) => setGroupBy(e.target.value as GroupBy)}
          className="rounded border border-gray-300 bg-transparent px-1 py-0.5 dark:border-gray-700"
        >
          <option value="none">None</option>
          <option value="project">Project</option>
          <option value="entrypoint">Entrypoint</option>
        </select>
      </label>
      <label className="flex items-center gap-1 text-xs">
        <input type="checkbox" checked={showExited} onChange={(e) => setShowExited(e.target.checked)} />
        Show exited
      </label>
    </div>
  );
}
