import { revealInFinder } from "../lib/ipc";
import { fmtTokens } from "../lib/format";
import { useStore } from "../store";
import type { Tokens } from "../types";
import { TranscriptView } from "./TranscriptView";

function TokenLine({ t }: { t: Tokens }) {
  return (
    <span>
      in {fmtTokens(t.input)} / out {fmtTokens(t.output)} / cache write {fmtTokens(t.cacheWrite)} /
      cache read {fmtTokens(t.cacheRead)}
    </span>
  );
}

function Field({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-2">
      <dt className="w-16 shrink-0 text-gray-500">{k}</dt>
      <dd className="min-w-0 break-all">{children}</dd>
    </div>
  );
}

export function DetailPanel() {
  const selected = useStore((s) => s.selected);
  const agent = useStore((s) => (s.selected ? s.agents[s.selected.agentId] : undefined));
  if (!selected || !agent) {
    return <div className="p-4 text-sm text-gray-500">Select an agent</div>;
  }
  const sub = selected.subagentId
    ? agent.subagents.find((x) => x.id === selected.subagentId)
    : undefined;

  return (
    <div className="flex h-full flex-col">
      <header className="space-y-1 border-b border-gray-200 p-3 text-xs dark:border-gray-800">
        <div className="text-sm font-semibold">{sub ? sub.description : agent.title}</div>
        <dl className="space-y-0.5">
          {sub ? (
            <>
              <Field k="type">{sub.agentType}</Field>
              <Field k="status">{sub.status}</Field>
              <Field k="tokens">
                <TokenLine t={sub.tokens} />
              </Field>
              <Field k="cwd">{agent.cwd}</Field>
            </>
          ) : (
            <>
              <Field k="cwd">{agent.cwd}</Field>
              <Field k="pid">{agent.pid}</Field>
              <Field k="model">{agent.model ?? "unknown"}</Field>
              <Field k="started">{new Date(agent.startedAt).toLocaleString()}</Field>
              <Field k="tokens">
                <TokenLine t={agent.tokens} />
              </Field>
              {import.meta.env.DEV && <Field k="skipped">{agent.skippedLines}</Field>}
            </>
          )}
        </dl>
        <button
          type="button"
          className="rounded border border-gray-300 px-2 py-0.5 dark:border-gray-700"
          onClick={() => void revealInFinder(agent.cwd).catch(() => {})}
        >
          Open folder in Finder
        </button>
      </header>
      <TranscriptView agentId={agent.id} subagentId={selected.subagentId} />
    </div>
  );
}
