import { useApp } from "../stores/app";

const colors = {
  info: "border-line bg-panel-2",
  success: "border-spotify/50 bg-panel-2",
  error: "border-red-500/60 bg-panel-2",
};

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  if (toasts.length === 0) return null;
  return (
    <div className="pointer-events-none fixed right-4 top-4 z-50 flex w-96 flex-col gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          onClick={() => dismiss(t.id)}
          className={`pointer-events-auto cursor-pointer rounded-md border px-4 py-3 text-sm shadow-lg ${colors[t.kind]}`}
        >
          {t.text}
        </div>
      ))}
    </div>
  );
}
