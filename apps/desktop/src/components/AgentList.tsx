import { groupAgents, sortAgents, visibleAgents } from "../lib/view";
import { useStore } from "../store";
import { AgentCard } from "./AgentCard";
import { EmptyState } from "./EmptyState";

export function AgentList({ now, claudeDir }: { now: Date; claudeDir: string }) {
  const agents = useStore((s) => s.agents);
  const filter = useStore((s) => s.filter);
  const showExited = useStore((s) => s.showExited);
  const search = useStore((s) => s.search);
  const groupBy = useStore((s) => s.groupBy);
  const list = sortAgents(visibleAgents({ agents, filter, showExited, search }, now));

  if (Object.keys(agents).length === 0) return <EmptyState claudeDir={claudeDir} />;
  if (list.length === 0) {
    return <p className="p-6 text-center text-sm text-gray-500">No matching sessions</p>;
  }
  return (
    <div className="h-full overflow-y-auto">
      {groupAgents(list, groupBy).map(([name, items]) => (
        <section key={name}>
          {groupBy !== "none" && (
            <h2 className="sticky top-0 bg-gray-100 px-3 py-1 text-xs font-semibold uppercase tracking-wide text-gray-500 dark:bg-gray-900">
              {name}
            </h2>
          )}
          {items.map((a) => (
            <AgentCard key={a.id} agent={a} now={now} />
          ))}
        </section>
      ))}
    </div>
  );
}
