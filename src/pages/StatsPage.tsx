import { PageHeader } from "../components/PlaylistPicker";

/**
 * Stats will be built on Spotify's account data export (full streaming
 * history). The polling-based dashboard was removed: 30-second snapshots are
 * too coarse to produce statistics worth showing.
 */
export function StatsPage() {
  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Stats" />
      <div className="flex-1 overflow-auto p-6">
        <section className="max-w-2xl rounded-lg border border-line bg-panel p-6">
          <h2 className="text-base font-semibold">Spotify data export</h2>
          <p className="mt-2 text-sm text-muted">
            Spotify can send you your complete streaming history: Account → Privacy settings → Download your data,
            with the extended streaming history option. It arrives by email, usually within 30 days, as a set of
            JSON files.
          </p>
          <p className="mt-3 text-xs text-muted">Not available yet</p>
          <button
            disabled
            className="mt-4 cursor-not-allowed rounded-md border border-line px-3 py-1.5 text-xs text-muted opacity-60"
          >
            Import export… (coming soon)
          </button>
        </section>
      </div>
    </div>
  );
}
