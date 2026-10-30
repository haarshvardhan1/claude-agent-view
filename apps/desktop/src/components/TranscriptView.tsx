import { useLayoutEffect, useRef, useState } from "react";
import { useTranscript, type ViewEntry } from "../lib/useTranscript";
import { useStore } from "../store";

const NEAR_BOTTOM_PX = 40;

function ToolCallRow({ e }: { e: Extract<ViewEntry, { kind: "toolCall" }> }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="rounded border border-gray-200 text-xs dark:border-gray-800">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-2 py-1 text-left font-mono"
      >
        <span>{open ? "▾" : "▸"}</span>
        <span className="min-w-0 flex-1 truncate">{`${e.name}: ${e.summary}`}</span>
        {e.result === null && <span className="text-gray-500">running…</span>}
      </button>
      {open && (
        <div className="space-y-1 border-t border-gray-200 p-2 dark:border-gray-800">
          <pre className="min-w-0 max-h-64 overflow-auto whitespace-pre-wrap break-words">{e.input}</pre>
          {e.result !== null && (
            <pre
              className={`min-w-0 max-h-64 overflow-auto whitespace-pre-wrap break-words ${
                e.isError ? "text-red-600 dark:text-red-400" : "text-gray-600 dark:text-gray-400"
              }`}
              data-error={e.isError || undefined}
            >
              {e.result}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

function ThinkingRow({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="text-xs text-gray-500">
      <button type="button" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
        {open ? "▾" : "▸"} Thinking
      </button>
      {open && <div className="mt-1 min-w-0 whitespace-pre-wrap break-words italic">{text}</div>}
    </div>
  );
}

function Row({ e }: { e: ViewEntry }) {
  switch (e.kind) {
    case "userPrompt":
      return (
        <div className="min-w-0 whitespace-pre-wrap break-words rounded bg-blue-50 p-2 text-sm dark:bg-blue-950">
          {e.text}
        </div>
      );
    case "assistantText":
      return <div className="min-w-0 whitespace-pre-wrap break-words p-2 text-sm">{e.text}</div>;
    case "thinking":
      return <ThinkingRow text={e.text} />;
    case "toolCall":
      return <ToolCallRow e={e} />;
  }
}

/** Keys that survive prepends and appends so expanded rows keep their state. */
function entryKeys(entries: ViewEntry[]): [string, ViewEntry][] {
  const seen = new Map<string, number>();
  return entries.map((e) => {
    const base = e.kind === "toolCall" ? `t|${e.id}` : `${e.kind}|${e.timestamp}|${e.text.length}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return [`${base}|${n}`, e];
  });
}

export function TranscriptView({ agentId, subagentId }: { agentId: string; subagentId?: string }) {
  const changeKey = useStore((s) => s.agents[agentId]);
  const t = useTranscript(agentId, subagentId, changeKey);
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const prevHeight = useRef(0);

  useLayoutEffect(() => {
    stick.current = true; // new selection starts pinned to the bottom
  }, [agentId, subagentId]);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !t.view || t.op === "none") return;
    if (t.op === "prepend") {
      el.scrollTop += el.scrollHeight - prevHeight.current;
    } else if (t.op === "replace" || stick.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [t.seq]); // eslint-disable-line react-hooks/exhaustive-deps

  if (t.notFound) {
    const gone =
      !changeKey ||
      changeKey.status.kind === "exited" ||
      (subagentId !== undefined && !changeKey.subagents.some((s) => s.id === subagentId));
    return (
      <div className="p-4 text-sm text-gray-500">
        {gone ? "This session is no longer available" : "No transcript yet"}
      </div>
    );
  }
  if (t.loading) return <div className="p-4 text-sm text-gray-500">Loading…</div>;
  if (!t.view) {
    return <div className="p-4 text-sm text-red-600">{t.error ?? "Could not load transcript"}</div>;
  }

  return (
    <div
      ref={ref}
      data-testid="transcript"
      className="min-h-0 flex-1 space-y-2 overflow-y-auto p-3"
      onScroll={(ev) => {
        const el = ev.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR_BOTTOM_PX;
      }}
    >
      {t.view.hasMore && (
        <button
          type="button"
          disabled={t.loadingEarlier}
          className="rounded border border-gray-300 px-2 py-1 text-xs disabled:opacity-50 dark:border-gray-700"
          onClick={() => {
            prevHeight.current = ref.current?.scrollHeight ?? 0;
            void t.loadEarlier();
          }}
        >
          Load earlier
        </button>
      )}
      {t.error && <div className="text-xs text-red-600">{t.error}</div>}
      {entryKeys(t.view.entries).map(([key, e]) => (
        <Row key={key} e={e} />
      ))}
    </div>
  );
}
