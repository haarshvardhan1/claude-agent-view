import { fmtDuration } from "../lib/format";
import { useStore } from "../store";
import type { Agent } from "../types";

export function SubagentTree({ agent, now }: { agent: Agent; now: Date }) {
  const select = useStore((s) => s.select);
  const selected = useStore((s) => s.selected);
  if (agent.subagents.length === 0) {
    return <div className="py-1 pl-6 text-xs text-gray-500">No subagents</div>;
  }
  return (
    <ul className="mt-1 border-l border-gray-300 pl-3 dark:border-gray-600">
      {agent.subagents.map((s) => {
        const active = selected?.agentId === agent.id && selected.subagentId === s.id;
        return (
          <li key={s.id}>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                select({ agentId: agent.id, subagentId: s.id });
              }}
              className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left text-xs hover:bg-black/5 dark:hover:bg-white/10 ${active ? "bg-black/10 dark:bg-white/15" : ""}`}
            >
              <span className="font-medium">{s.agentType}</span>
              <span className="min-w-0 flex-1 truncate text-gray-600 dark:text-gray-300">{s.description}</span>
              <span className="text-gray-500">{s.status}</span>
              <span className="w-10 text-right text-gray-500">
                {s.lastActivity ? fmtDuration(now.getTime() - Date.parse(s.lastActivity)) : "–"}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
