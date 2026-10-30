// Mirrors crates/av-core/src/model.rs and src-tauri settings (camelCase wire format).
export type Iso = string; // RFC 3339

export type Status =
  | { kind: "exited" }
  | { kind: "needsPermission"; tool: string | null }
  | { kind: "error"; errorKind: string | null }
  | { kind: "waitingInput" }
  | { kind: "maybeWaiting" }
  | { kind: "working" }
  | { kind: "idle" }
  | { kind: "unknown"; reason: string };

export interface Context {
  used: number | null;
  limit: number | null;
}

export interface Tokens {
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
}

export interface PendingTool {
  name: string;
  since: Iso;
}

export type SubagentState = "running" | "done" | "stopped";

export interface Subagent {
  id: string;
  agentType: string;
  description: string;
  background: boolean;
  status: SubagentState;
  lastActivity: Iso | null;
  tokens: Tokens;
  pendingTool: PendingTool | null;
  transcriptPath: string;
}

export interface Agent {
  id: string;
  pid: number;
  cwd: string;
  projectName: string;
  gitBranch: string | null;
  title: string;
  entrypoint: string;
  model: string | null;
  status: Status;
  statusSince: Iso;
  startedAt: Iso;
  lastActivity: Iso | null;
  context: Context;
  tokens: Tokens;
  pendingTool: PendingTool | null;
  subagents: Subagent[];
  transcriptPath: string | null;
  skippedLines: number;
}

export type Change =
  | { type: "agentAdded"; agent: Agent }
  | { type: "agentUpdated"; agent: Agent }
  | { type: "agentRemoved"; id: string };

export type TranscriptEntry =
  | { kind: "userPrompt"; text: string; timestamp: Iso | null }
  | { kind: "assistantText"; text: string; timestamp: Iso | null }
  | { kind: "thinking"; text: string; timestamp: Iso | null }
  | {
      kind: "toolCall";
      id: string;
      name: string;
      summary: string;
      input: string;
      result: string | null;
      isError: boolean;
      timestamp: Iso | null;
    }
  | {
      kind: "toolResult";
      toolUseId: string;
      result: string;
      isError: boolean;
      timestamp: Iso | null;
    };

export interface TranscriptPage {
  entries: TranscriptEntry[];
  startOffset: number;
  endOffset: number;
}

export interface NotifySettings {
  needsPermission: boolean;
  waitingInput: boolean;
  finished: boolean;
  error: boolean;
  subagentFinished: boolean;
  finishedAfterSecs: number;
}

export interface AppSettings {
  notify: NotifySettings;
  launchAtLogin: boolean;
  claudeDir: string | null;
  hooksBannerDismissed: boolean;
}

export type HooksStatus = "installed" | "notInstalled" | "broken";

export interface HookReport {
  backup: string | null;
  changed: boolean;
}

export type Selection = { agentId: string; subagentId?: string } | null;
export type Filter = "all" | "needsYou" | "working" | "idle" | "exited";
export type GroupBy = "none" | "project" | "entrypoint";
export type StatusGroup = "needsYou" | "working" | "idle" | "exited";
