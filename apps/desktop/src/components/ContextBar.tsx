import { contextLevel, fmtTokens } from "../lib/format";
import type { Context } from "../types";

const COLORS = { ok: "bg-sky-500", warn: "bg-amber-500", high: "bg-red-500" };

export function ContextBar({ context }: { context: Context }) {
  if (context.used == null) return null;
  const { used, limit } = context;
  const level = contextLevel(used, limit);
  const pct = limit ? Math.min(100, (used / limit) * 100) : 0;
  return (
    <div data-testid="context" className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400">
      {limit != null && (
        <div className="h-1.5 w-24 overflow-hidden rounded bg-gray-200 dark:bg-gray-700">
          <div
            data-testid="context-fill"
            data-level={level}
            className={`h-full ${COLORS[level]}`}
            style={{ width: `${pct}%` }}
          />
        </div>
      )}
      <span>{limit != null ? `${fmtTokens(used)} / ${fmtTokens(limit)}` : fmtTokens(used)}</span>
    </div>
  );
}
