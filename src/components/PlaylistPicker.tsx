import { useMemo, useState } from "react";
import { useApp } from "../stores/app";

const inputClass =
  "rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify disabled:opacity-50";

/** Single playlist dropdown. */
export function PlaylistSelect({
  value,
  onChange,
  placeholder = "Choose a playlist…",
  exclude,
  disabled,
  className = "",
}: {
  value: string | null;
  onChange: (id: string | null) => void;
  placeholder?: string;
  exclude?: string | null;
  disabled?: boolean;
  className?: string;
}) {
  const playlists = useApp((s) => s.playlists);
  return (
    <select
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || null)}
      disabled={disabled}
      className={`${inputClass} min-w-64 ${className}`}
    >
      <option value="">{placeholder}</option>
      {playlists
        .filter((p) => p.id !== exclude)
        .map((p) => (
          <option key={p.id} value={p.id}>
            {p.name} ({p.trackCount})
          </option>
        ))}
    </select>
  );
}

/** Checkbox list with filter for picking several playlists. */
export function PlaylistMultiSelect({
  selected,
  onChange,
  disabled,
  maxHeight = "max-h-64",
}: {
  selected: Set<string>;
  onChange: (next: Set<string>) => void;
  disabled?: boolean;
  maxHeight?: string;
}) {
  const playlists = useApp((s) => s.playlists);
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? playlists.filter((p) => p.name.toLowerCase().includes(q)) : playlists;
  }, [playlists, query]);

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onChange(next);
  }

  return (
    <div className="rounded-md border border-line bg-ink">
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter playlists"
          disabled={disabled}
          className="flex-1 bg-transparent text-sm outline-none"
        />
        <span className="text-xs text-muted">{selected.size} selected</span>
        {selected.size > 0 && (
          <button className="text-xs text-muted hover:text-white" onClick={() => onChange(new Set())} disabled={disabled}>
            Clear
          </button>
        )}
      </div>
      <ul className={`${maxHeight} overflow-auto`}>
        {filtered.map((p) => (
          <li key={p.id}>
            <label className="flex cursor-pointer items-center gap-3 px-3 py-1.5 text-sm hover:bg-panel-2">
              <input type="checkbox" checked={selected.has(p.id)} onChange={() => toggle(p.id)} disabled={disabled} />
              <span className="min-w-0 flex-1 truncate">{p.name}</span>
              <span className="shrink-0 text-xs text-muted">{p.trackCount}</span>
            </label>
          </li>
        ))}
        {filtered.length === 0 && <li className="px-3 py-3 text-sm text-muted">No playlists match.</li>}
      </ul>
    </div>
  );
}

export function PageHeader({ title, subtitle, children }: { title: string; subtitle?: string; children?: React.ReactNode }) {
  return (
    <header className="flex items-center gap-3 border-b border-line px-6 py-4">
      <h1 className="text-xl font-bold">{title}</h1>
      {subtitle && <span className="text-sm text-muted">{subtitle}</span>}
      <div className="ml-auto flex items-center gap-2">{children}</div>
    </header>
  );
}
