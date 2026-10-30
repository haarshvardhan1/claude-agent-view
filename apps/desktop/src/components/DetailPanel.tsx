import { useStore } from "../store";

/** Placeholder; Task 16 fills this in. */
export function DetailPanel() {
  const title = useStore((s) => (s.selected ? s.agents[s.selected.agentId]?.title : undefined));
  return <div className="p-4 text-sm">{title ?? "Select an agent"}</div>;
}
