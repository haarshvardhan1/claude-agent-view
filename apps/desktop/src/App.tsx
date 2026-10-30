import { useEffect, useState } from "react";
import { AgentList } from "./components/AgentList";
import { DetailPanel } from "./components/DetailPanel";
import { HooksBanner } from "./components/HooksBanner";
import { SettingsView } from "./components/SettingsView";
import { Toasts } from "./components/Toasts";
import { Toolbar } from "./components/Toolbar";
import { call, on } from "./lib/ipc";
import { useStore } from "./store";
import type { Agent, AppSettings, Change } from "./types";

function useNow(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(t);
  }, []);
  return now;
}

function MainWindow() {
  const now = useNow();
  const [claudeDir, setClaudeDir] = useState("~/.claude");

  useEffect(() => {
    let cancelled = false;
    const unlisten: (() => void)[] = [];
    const { setAll, apply, select } = useStore.getState();
    // Subscribe first so no change is missed while the initial list loads.
    on<Change[]>("agents://changed", apply).then((u) => (cancelled ? u() : unlisten.push(u)));
    on<string>("ui://select-agent", (id) => select({ agentId: id })).then((u) =>
      cancelled ? u() : unlisten.push(u),
    );
    call<Agent[]>("list_agents").then(setAll).catch(() => {});
    call<AppSettings>("get_settings")
      .then((s) => s.claudeDir && setClaudeDir(s.claudeDir))
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten.forEach((u) => u());
    };
  }, []);

  return (
    <div className="flex h-screen flex-col bg-white text-gray-900 dark:bg-gray-950 dark:text-gray-100">
      <HooksBanner />
      <Toolbar now={now} />
      <div className="flex min-h-0 flex-1">
        <div className="w-[420px] shrink-0 border-r border-gray-200 dark:border-gray-800">
          <AgentList now={now} claudeDir={claudeDir} />
        </div>
        <div className="min-w-0 flex-1 overflow-hidden">
          <DetailPanel />
        </div>
      </div>
      <Toasts />
    </div>
  );
}

function App() {
  if (window.location.hash === "#settings") {
    return <SettingsView />;
  }
  return <MainWindow />;
}

export default App;
