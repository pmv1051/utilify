import { useMemo, useState } from "react";

/**
 * Small chart kit for the Stats page. Every chart here plots one series, so
 * there is one colour and no legend: the card title says what is plotted.
 * Bars carry the colour, text never does.
 *
 * Each chart has a table twin behind the "Table" toggle, so no value is only
 * reachable by hovering.
 */

export interface ChartPoint {
  /** Row label in the table view. */
  label: string;
  value: number;
  /** Tooltip heading, e.g. "Tuesday". Defaults to `label`. */
  title?: string;
  /** Second tooltip line, e.g. the play count behind the time. */
  detail?: string;
  /** Axis tick under this band; bands without one stay unlabelled. */
  tick?: string;
}

/** Round a maximum up to a readable step, close enough that the tallest bar
 *  still fills most of the plot. */
function niceMax(value: number): number {
  if (value <= 0) return 1;
  const exponent = Math.floor(Math.log10(value));
  const base = 10 ** exponent;
  const n = value / base;
  const step = [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].find((s) => n <= s + 1e-9) ?? 10;
  return step * base;
}

function CardShell({
  title,
  subtitle,
  showTable,
  onToggleTable,
  children,
}: {
  title: string;
  subtitle?: string;
  showTable: boolean;
  onToggleTable: () => void;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-line bg-panel p-4">
      <header className="mb-4 flex items-start gap-3">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-zinc-100">{title}</h3>
          {subtitle && <p className="mt-0.5 text-xs text-muted">{subtitle}</p>}
        </div>
        <button
          type="button"
          onClick={onToggleTable}
          aria-pressed={showTable}
          className={`ml-auto shrink-0 rounded border border-line px-2 py-1 text-[11px] transition hover:bg-panel-2 ${
            showTable ? "text-zinc-100" : "text-muted"
          }`}
        >
          Table
        </button>
      </header>
      {children}
    </section>
  );
}

type LabelledPoint = ChartPoint & { valueLabel: string };

function PointTable({
  points,
  bucketHeading,
  valueHeading,
  detailHeading,
}: {
  points: LabelledPoint[];
  bucketHeading: string;
  valueHeading: string;
  detailHeading: string;
}) {
  return (
    <div className="max-h-64 overflow-auto">
      <table className="w-full text-xs">
        <thead className="sticky top-0 bg-panel text-muted">
          <tr>
            <th className="py-1 text-left font-medium">{bucketHeading}</th>
            <th className="py-1 text-right font-medium">{valueHeading}</th>
            <th className="py-1 text-right font-medium">{detailHeading}</th>
          </tr>
        </thead>
        <tbody className="text-zinc-300">
          {points.map((p) => (
            <tr key={p.label} className="border-t border-line/60">
              <td className="py-1 pr-2">{p.title ?? p.label}</td>
              <td className="py-1 text-right tabular-nums">{p.valueLabel}</td>
              <td className="py-1 pl-2 text-right text-muted">{p.detail ?? ""}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/**
 * Column chart for ordered buckets (hours of the day, weekdays, months).
 * Bars grow from a single baseline and are capped at 24px so a wide card
 * leaves air between them rather than fat blocks.
 */
export function Columns({
  title,
  subtitle,
  points,
  formatValue,
  formatTick,
  bucketHeading,
  valueHeading,
  detailHeading,
  unit = 1,
  height = 176,
}: {
  title: string;
  subtitle?: string;
  points: ChartPoint[];
  /** Full, unambiguous value for the tooltip and the table, e.g. "3 h 12 min". */
  formatValue: (value: number) => string;
  /** Short form for axis ticks, where the heading carries the unit. */
  formatTick: (value: number) => string;
  bucketHeading: string;
  valueHeading: string;
  detailHeading: string;
  /** Size of one display unit (e.g. an hour in ms) so axis ticks round to
   *  whole units rather than to round numbers of milliseconds. */
  unit?: number;
  height?: number;
}) {
  const [showTable, setShowTable] = useState(false);
  const [active, setActive] = useState<number | null>(null);

  const max = useMemo(
    () => niceMax(Math.max(0, ...points.map((p) => p.value)) / unit) * unit,
    [points, unit],
  );
  const labelled = useMemo(
    () => points.map((p) => ({ ...p, valueLabel: formatValue(p.value) })),
    [points, formatValue],
  );

  if (showTable) {
    return (
      <CardShell title={title} subtitle={subtitle} showTable onToggleTable={() => setShowTable(false)}>
        <PointTable
          points={labelled}
          bucketHeading={bucketHeading}
          valueHeading={valueHeading}
          detailHeading={detailHeading}
        />
      </CardShell>
    );
  }

  const hovered = active === null ? null : labelled[active] ?? null;

  return (
    <CardShell title={title} subtitle={subtitle} showTable={false} onToggleTable={() => setShowTable(true)}>
      <div className="relative">
        {hovered && (
          <div
            className="pointer-events-none absolute -top-1 z-10 -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-md border border-line bg-ink px-2 py-1 shadow-lg"
            style={{ left: `calc(3rem + (100% - 3rem) * ${((active ?? 0) + 0.5) / labelled.length})` }}
          >
            <div className="text-xs font-semibold text-zinc-100">{hovered.valueLabel}</div>
            <div className="text-[11px] text-muted">{hovered.title ?? hovered.label}</div>
            {hovered.detail && <div className="text-[11px] text-muted">{hovered.detail}</div>}
          </div>
        )}

        <div className="relative" style={{ height }}>
          {/* Hairline gridlines, one step off the surface. */}
          {[0, 0.25, 0.5, 0.75, 1].map((f) => (
            <div key={f} className="absolute left-12 right-0 border-t border-line" style={{ top: `${(1 - f) * 100}%` }} />
          ))}
          {[0, 0.5, 1].map((f) => (
            <div
              key={f}
              className="absolute left-0 w-10 -translate-y-1/2 text-right text-[10px] tabular-nums text-muted"
              style={{ top: `${(1 - f) * 100}%` }}
            >
              {formatTick(max * f)}
            </div>
          ))}

          <div className="absolute inset-y-0 left-12 right-0 flex items-end gap-[2px]">
            {labelled.map((p, i) => (
              <button
                key={p.label}
                type="button"
                className="group relative flex h-full flex-1 items-end justify-center"
                onMouseEnter={() => setActive(i)}
                onMouseLeave={() => setActive((cur) => (cur === i ? null : cur))}
                onFocus={() => setActive(i)}
                onBlur={() => setActive((cur) => (cur === i ? null : cur))}
                aria-label={`${p.title ?? p.label}: ${p.valueLabel}`}
              >
                <span
                  className={`w-full max-w-[24px] rounded-t-[4px] bg-spotify transition-[filter] ${
                    active === i ? "brightness-125" : "group-hover:brightness-125"
                  }`}
                  style={{ height: `${Math.max(p.value > 0 ? 1 : 0, (p.value / max) * 100)}%` }}
                />
              </button>
            ))}
          </div>
        </div>

        <div className="mt-1.5 flex gap-[2px] pl-12">
          {labelled.map((p) => (
            <div key={p.label} className="flex-1 text-center text-[10px] tabular-nums text-muted">
              {p.tick ?? ""}
            </div>
          ))}
        </div>
      </div>
    </CardShell>
  );
}

export interface RankRow {
  key: string;
  primary: string;
  secondary?: string;
  /** Drives the bar length. */
  value: number;
  valueLabel: string;
  /** Muted right-hand column, e.g. "42 plays". */
  extra?: string;
}

/**
 * Ranked horizontal bars. The names are long, so this is a table with the
 * magnitude drawn beside each row rather than a chart with a name axis; that
 * also makes it its own table view.
 */
export function RankedList({
  title,
  subtitle,
  rows,
  empty,
  initial = 10,
}: {
  title: string;
  subtitle?: string;
  rows: RankRow[];
  empty: string;
  initial?: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const max = Math.max(1, ...rows.map((r) => r.value));
  const shown = expanded ? rows : rows.slice(0, initial);
  const hasExtra = rows.some((r) => r.extra);

  return (
    <section className="rounded-lg border border-line bg-panel p-4">
      <header className="mb-3">
        <h3 className="text-sm font-semibold text-zinc-100">{title}</h3>
        {subtitle && <p className="mt-0.5 text-xs text-muted">{subtitle}</p>}
      </header>

      {rows.length === 0 ? (
        <p className="py-6 text-center text-xs text-muted">{empty}</p>
      ) : (
        <>
          <table className="w-full table-fixed text-xs">
            <colgroup>
              <col className="w-7" />
              <col />
              <col className="w-[30%]" />
              <col className="w-16" />
              {hasExtra && <col className="w-24" />}
            </colgroup>
            <tbody>
              {shown.map((r, i) => (
                <tr key={r.key} className="border-t border-line/60 first:border-t-0">
                  <td className="py-1.5 pr-2 text-right tabular-nums text-muted">{i + 1}</td>
                  <td className="py-1.5 pr-3">
                    <div className="truncate text-zinc-100" title={r.primary}>
                      {r.primary}
                    </div>
                    {r.secondary && (
                      <div className="truncate text-[11px] text-muted" title={r.secondary}>
                        {r.secondary}
                      </div>
                    )}
                  </td>
                  <td className="py-1.5 pr-3">
                    <div className="h-2 w-full">
                      <div
                        className="h-2 rounded-r-[4px] bg-spotify"
                        style={{ width: r.value > 0 ? `${Math.max(2, (r.value / max) * 100)}%` : 0 }}
                      />
                    </div>
                  </td>
                  <td className="whitespace-nowrap py-1.5 text-right tabular-nums text-zinc-100">
                    {r.valueLabel}
                  </td>
                  {hasExtra && (
                    <td className="whitespace-nowrap py-1.5 pl-3 text-right tabular-nums text-muted">
                      {r.extra ?? ""}
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length > initial && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="mt-2 text-[11px] text-muted underline-offset-2 hover:text-zinc-200 hover:underline"
            >
              {expanded ? "Show top 10" : `Show all ${rows.length}`}
            </button>
          )}
        </>
      )}
    </section>
  );
}

export function StatTile({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-lg border border-line bg-panel px-4 py-3">
      <div className="text-xs text-muted">{label}</div>
      <div className="mt-1 text-2xl font-semibold text-zinc-100">{value}</div>
      {hint && <div className="mt-0.5 text-[11px] text-muted">{hint}</div>}
    </div>
  );
}
