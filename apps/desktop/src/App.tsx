import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export function AgentCount({ count }: { count: number | null }) {
  return (
    <p className="text-sm text-gray-600">
      {count === null ? "Loading agents…" : `${count} agent${count === 1 ? "" : "s"}`}
    </p>
  );
}

function App() {
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    invoke<unknown[]>("list_agents")
      .then((agents) => setCount(agents.length))
      .catch(() => setCount(0));
  }, []);

  return (
    <main className="p-6">
      <h1 className="text-2xl font-semibold">Agent Viewer</h1>
      <AgentCount count={count} />
    </main>
  );
}

export default App;
