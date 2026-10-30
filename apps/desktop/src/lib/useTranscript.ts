import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "./ipc";
import type { TranscriptEntry, TranscriptPage } from "../types";

export const PAGE_LIMIT = 200;

type PendingResults = Record<string, { result: string; isError: boolean }>;
export type ViewEntry = Exclude<TranscriptEntry, { kind: "toolResult" }>;
export type Op = "replace" | "append" | "prepend" | "none";

export interface TranscriptView {
  entries: ViewEntry[];
  startOffset: number;
  endOffset: number;
  hasMore: boolean;
  pending: PendingResults;
}

export interface TranscriptState {
  view: TranscriptView | null;
  loading: boolean;
  notFound: boolean;
  error: string | null;
  loadingEarlier: boolean;
  /** Which kind of change produced the current view (drives scrolling). */
  op: Op;
  seq: number;
}

/** Merge raw entries into a normalized list: toolResult rows are folded into their toolCall. */
export function normalize(
  raw: TranscriptEntry[],
  pending: PendingResults,
): { entries: ViewEntry[]; pending: PendingResults } {
  const results: PendingResults = { ...pending };
  for (const e of raw) {
    if (e.kind === "toolResult") results[e.toolUseId] = { result: e.result, isError: e.isError };
  }
  const entries: ViewEntry[] = [];
  const used = new Set<string>();
  for (const e of raw) {
    if (e.kind === "toolResult") continue;
    if (e.kind === "toolCall" && results[e.id]) {
      used.add(e.id);
      entries.push({ ...e, result: results[e.id].result, isError: results[e.id].isError });
    } else {
      entries.push(e);
    }
  }
  const left: PendingResults = {};
  for (const [id, r] of Object.entries(results)) if (!used.has(id)) left[id] = r;
  return { entries, pending: left };
}

const errCode = (e: unknown): string | undefined =>
  typeof e === "object" && e !== null ? (e as { code?: string }).code : undefined;
const errMsg = (e: unknown): string =>
  typeof e === "object" && e !== null && "message" in e
    ? String((e as { message: unknown }).message)
    : String(e);

interface Ctl {
  inflight: boolean;
  dirty: boolean;
  /** A tail fetch is in flight. */
  tail: boolean;
}

/**
 * Transcript state for one selection. `changeKey` should change whenever the
 * selected agent receives an update (the store replaces the agent object).
 */
export function useTranscript(agentId: string, subagentId: string | undefined, changeKey: unknown) {
  const [state, setState] = useState<TranscriptState>({
    view: null,
    loading: true,
    notFound: false,
    error: null,
    loadingEarlier: false,
    op: "replace",
    seq: 0,
  });
  const stateRef = useRef(state);
  const tokenRef = useRef(0);
  // Bumped on every view replace and reset; drops stale `before` pages.
  const epochRef = useRef(0);
  const earlierBusy = useRef(false);
  const ctlRef = useRef<Ctl>({ inflight: false, dirty: false, tail: false });
  const commit = useCallback((patch: Partial<TranscriptState>) => {
    stateRef.current = { ...stateRef.current, ...patch, seq: stateRef.current.seq + 1 };
    setState(stateRef.current);
  }, []);

  const fetchPage = useCallback(
    (req: { kind: "tail" } | { kind: "before" | "after"; offset: number }) =>
      call<TranscriptPage>("get_transcript", { agentId, subagentId: subagentId ?? null, req }),
    [agentId, subagentId],
  );

  const fail = useCallback(
    (e: unknown) => {
      if (errCode(e) === "notFound") commit({ notFound: true, loading: false, loadingEarlier: false, op: "none" });
      else commit({ error: errMsg(e), loading: false, loadingEarlier: false, op: "none" });
    },
    [commit],
  );

  const runAfter = useCallback(
    async (tok: number, ctl: Ctl) => {
      if (ctl.inflight) {
        ctl.dirty = true;
        return;
      }
      ctl.inflight = true;
      try {
        do {
          ctl.dirty = false;
          for (;;) {
            const v = stateRef.current.view;
            if (!v) break;
            const page = await fetchPage({ kind: "after", offset: v.endOffset });
            if (tok !== tokenRef.current) return;
            const cur = stateRef.current.view ?? v;
            if (page.startOffset < cur.endOffset) {
              // File truncated or replaced: the page is a Tail page.
              const n = normalize(page.entries, {});
              epochRef.current++;
              commit({
                view: {
                  entries: n.entries,
                  startOffset: page.startOffset,
                  endOffset: page.endOffset,
                  hasMore: page.startOffset > 0,
                  pending: n.pending,
                },
                op: "replace",
                error: null,
              });
              break;
            }
            const n = normalize([...cur.entries, ...page.entries], cur.pending);
            commit({
              view: { ...cur, entries: n.entries, endOffset: page.endOffset, pending: n.pending },
              op: "append",
              error: null,
            });
            if (!(page.entries.length >= PAGE_LIMIT && page.endOffset > cur.endOffset)) break;
          }
        } while (ctl.dirty);
      } catch (e) {
        if (tok === tokenRef.current) fail(e);
      } finally {
        ctl.inflight = false;
      }
    },
    [fetchPage, commit, fail],
  );

  // Loads the tail into an empty view (first load, or a retry after notFound).
  const loadTail = useCallback(
    (tok: number, ctl: Ctl) => {
      ctl.tail = true;
      fetchPage({ kind: "tail" })
        .then((page) => {
          if (tok !== tokenRef.current) return;
          ctl.tail = false;
          const n = normalize(page.entries, {});
          epochRef.current++;
          commit({
            view: {
              entries: n.entries,
              startOffset: page.startOffset,
              endOffset: page.endOffset,
              hasMore: page.startOffset > 0,
              pending: n.pending,
            },
            loading: false,
            notFound: false,
            error: null,
            op: "replace",
          });
          if (ctl.dirty) void runAfter(tok, ctl);
        })
        .catch((e) => {
          if (tok !== tokenRef.current) return;
          ctl.tail = false;
          ctl.dirty = false;
          fail(e);
        });
    },
    [fetchPage, commit, fail, runAfter],
  );

  // Selection change: reset and load the tail.
  useEffect(() => {
    const tok = ++tokenRef.current;
    epochRef.current++;
    earlierBusy.current = false;
    const ctl: Ctl = { inflight: false, dirty: false, tail: false };
    ctlRef.current = ctl;
    stateRef.current = {
      view: null,
      loading: true,
      notFound: false,
      error: null,
      loadingEarlier: false,
      op: "replace",
      seq: stateRef.current.seq + 1,
    };
    setState(stateRef.current);
    loadTail(tok, ctl);
  }, [loadTail]);

  // Live updates: the selected agent changed.
  const seenSel = useRef(`${agentId}|${subagentId ?? ""}`);
  const first = useRef(true);
  useEffect(() => {
    const sel = `${agentId}|${subagentId ?? ""}`;
    if (first.current || seenSel.current !== sel) {
      // Mount or selection switch: the tail fetch covers it.
      first.current = false;
      seenSel.current = sel;
      return;
    }
    const ctl = ctlRef.current;
    if (!stateRef.current.view) {
      if (ctl.tail) {
        ctl.dirty = true; // tail still loading; refetch after it lands
      } else {
        // No view yet (e.g. notFound: the session appeared before its
        // transcript file): try the tail again.
        loadTail(tokenRef.current, ctl);
      }
      return;
    }
    void runAfter(tokenRef.current, ctl);
    // runAfter/loadTail intentionally omitted: they only change with the selection.
  }, [changeKey, agentId, subagentId]);

  const loadEarlier = useCallback(async () => {
    const v = stateRef.current.view;
    if (!v || !v.hasMore || earlierBusy.current) return;
    earlierBusy.current = true;
    commit({ loadingEarlier: true, op: "none" });
    const tok = tokenRef.current;
    const epoch = epochRef.current;
    try {
      const page = await fetchPage({ kind: "before", offset: v.startOffset });
      if (tok !== tokenRef.current) return;
      earlierBusy.current = false;
      if (epoch !== epochRef.current) {
        commit({ loadingEarlier: false, op: "none" });
        return;
      }
      const cur = stateRef.current.view ?? v;
      const n = normalize([...page.entries, ...cur.entries], cur.pending);
      commit({
        view: {
          ...cur,
          entries: n.entries,
          startOffset: page.startOffset,
          pending: n.pending,
          hasMore: page.entries.length > 0 && page.startOffset > 0,
        },
        loadingEarlier: false,
        op: "prepend",
      });
    } catch (e) {
      if (tok === tokenRef.current) {
        earlierBusy.current = false;
        fail(e);
      }
    }
  }, [fetchPage, commit, fail]);

  return { ...state, loadEarlier };
}
