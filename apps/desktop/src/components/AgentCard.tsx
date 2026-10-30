import { useState } from "react";
import { fmtDuration, statusLabel } from "../lib/format";
import { useStore } from "../store";
import type { Agent } from "../types";
import { ContextBar } from "./ContextBar";
import { SubagentTree } from "./SubagentTree";

function dotClass(kind: Agent["status"]["kind"]): string {
  switch (kind) {
    case "needsPermission":
    case "error":
      return "bg-red-500";
    case "waitingInput":
      return "bg-amber-500";
    case "working":
      return "bg-green-500";
    case "maybeWaiting":
      return "bg-gray-400 opacity-50";
    case "idle":
      return "bg-slate-400";
    default:
      return "bg-gray-400/50";
  }
}

export function AgentCard({ agent, now }: { agent: Agent; now: Date }) {
  const select = useStore((s) => s.select);
  const active = useStore(
    (s) => s.selected?.agentId === agent.id && !s.selected.subagentId,
  );
  const [open, setOpen] = useState(false);
  const faded = agent.status.kind === "maybeWaiting";
  const running = agent.subagents.filter((s) => s.status === "running").length;
  const since = fmtDuration(now.getTime() - Date.parse(agent.statusSince));
  const label = statusLabel(agent.status);

  return (
    <div
      className={`border-b border-gray-200 px-3 py-2 dark:border-gray-800 ${active ? "bg-blue-500/10" : ""}`}
    >
      <div
        role="button"
        tabIndex={0}
        onClick={() => select({ agentId: agent.id })}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            select({ agentId: agent.id });
          }
        }}
        className="cursor-pointer rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
      >
        <div className="flex items-center gap-2">
          <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${dotClass(agent.status.kind)}`} />
          <span data-testid="status-label" className={`text-xs font-medium ${faded ? "opacity-50" : ""}`}>
            {label}
          </span>
          <span className="ml-auto text-xs text-gray-500">{agent.entrypoint}</span>
        </div>
        <div className="mt-1 truncate text-sm font-semibold">{agent.title}</div>
        <div className="truncate text-xs text-gray-500 dark:text-gray-400">
          {agent.projectName}
          {agent.gitBranch ? ` · ${agent.gitBranch}` : ""}
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-gray-500 dark:text-gray-400">
          <span>{`${label.split(":")[0]} ${since}`}</span>
          {agent.pendingTool && (
            <span>{`${agent.pendingTool.name} · ${fmtDuration(now.getTime() - Date.parse(agent.pendingTool.since))}`}</span>
          )}
          <ContextBar context={agent.context} />
        </div>
      </div>
      {agent.subagents.length > 0 && (
        <div className="mt-1">
          <button
            type="button"
            aria-label={open ? "Collapse subagents" : "Expand subagents"}
            aria-expanded={open}
            onClick={() => setOpen(!open)}
            className="text-xs text-gray-500 hover:text-gray-800 dark:hover:text-gray-200"
          >
            <span aria-hidden>{open ? "▾" : "▸"}</span>{" "}
            <span>{running > 0 ? `${running} ${running === 1 ? "subagent" : "subagents"}` : `${agent.subagents.length} done`}</span>
          </button>
          {open && <SubagentTree agent={agent} now={now} />}
        </div>
      )}
    </div>
  );
}
