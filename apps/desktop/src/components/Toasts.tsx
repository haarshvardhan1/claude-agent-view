import { useStore } from "../store";

export function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismissToast);
  if (toasts.length === 0) return null;
  return (
    <div className="fixed bottom-4 right-4 z-50 flex max-w-sm flex-col gap-2" role="status">
      {toasts.map((t) => (
        <button
          key={t.id}
          type="button"
          onClick={() => dismiss(t.id)}
          className="rounded-md bg-gray-900 px-3 py-2 text-left text-sm text-white shadow-lg dark:bg-gray-100 dark:text-gray-900"
        >
          {t.message}
        </button>
      ))}
    </div>
  );
}
