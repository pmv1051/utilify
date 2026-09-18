export function Spinner({ className = "" }: { className?: string }) {
  return (
    <span
      className={`inline-block h-4 w-4 animate-spin rounded-full border-2 border-zinc-500 border-t-zinc-100 ${className}`}
      aria-label="Loading"
    />
  );
}
