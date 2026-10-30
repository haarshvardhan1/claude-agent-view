export function EmptyState({ claudeDir }: { claudeDir: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-1 p-6 text-center text-gray-500">
      <p className="text-sm font-medium">No Claude Code sessions running</p>
      <p className="text-xs">Watching {claudeDir}</p>
    </div>
  );
}
