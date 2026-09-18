import { useState } from "react";
import { api, errorMessage } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { useQuotaCooldown } from "../lib/quota";

export function SettingsPage() {
  const settings = useApp((s) => s.settings);
  const refreshSettings = useApp((s) => s.refreshSettings);
  const refreshQuota = useApp((s) => s.refreshQuota);
  const setSetup = useApp((s) => s.setSetup);
  const toast = useApp((s) => s.toast);
  const cooldown = useQuotaCooldown();
  const [busy, setBusy] = useState(false);
  const [threshold, setThreshold] = useState<string | null>(null);
  const [savingThreshold, setSavingThreshold] = useState(false);

  async function toggleTray(enabled: boolean) {
    try {
      await api.setMinimizeToTray(enabled);
      await refreshSettings();
    } catch (e) {
      toast("error", errorMessage(e));
    }
  }

  async function disconnect() {
    setBusy(true);
    try {
      await api.disconnect();
      setSetup(await api.getSetupState());
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  if (!settings) return <div className="p-6 text-muted">Loading…</div>;

  const thresholdInput = threshold ?? String(settings.playThresholdSecs);
  const thresholdValue = Number(thresholdInput);
  const thresholdValid = Number.isInteger(thresholdValue) && thresholdValue >= 1 && thresholdValue <= 600;

  async function saveThreshold() {
    if (!thresholdValid) return;
    setSavingThreshold(true);
    try {
      await api.setPlayThreshold(thresholdValue);
      await refreshSettings();
      setThreshold(null);
      toast("success", `A play now counts after ${thresholdValue} s.`);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setSavingThreshold(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="border-b border-line px-6 py-4">
        <h1 className="text-xl font-bold">Settings</h1>
      </header>
      <div className="flex-1 space-y-6 overflow-auto p-6">
        <Section title="Background">
          <label className="flex cursor-pointer items-start gap-3">
            <input
              type="checkbox"
              className="mt-1"
              checked={settings.minimizeToTray}
              onChange={(e) => toggleTray(e.target.checked)}
            />
            <span>
              <span className="block text-sm">Keep running in the system tray when the window is closed</span>
              <span className="block text-xs text-muted">
                Required for automatic re-shuffles and bench restores.
                Use Quit in the tray menu to fully exit.
              </span>
            </span>
          </label>
        </Section>

        <Section title="Spotify">
          <Row label="Account" value={settings.userDisplayName ?? settings.userId ?? "—"} />
          <Row label="Client ID" value={settings.clientId ?? "—"} mono />
          <Row label="Redirect URI" value={settings.redirectUri} mono />
          <div className="mt-3">
            <Button variant="danger" onClick={disconnect} disabled={busy}>
              Disconnect
            </Button>
            <span className="ml-3 text-xs text-muted">Removes the stored tokens. Your Client ID is kept.</span>
          </div>
        </Section>

        <Section title="Discovery">
          <div className="flex flex-wrap items-center gap-2 text-sm">
            A track counts as listened after
            <input
              type="number"
              min={1}
              max={600}
              value={thresholdInput}
              onChange={(e) => setThreshold(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && saveThreshold()}
              className="w-20 rounded-md border border-line bg-ink px-2 py-1 text-sm outline-none focus:border-spotify"
            />
            seconds; anything shorter is a skip.
            {thresholdValid && thresholdValue !== settings.playThresholdSecs && (
              <Button onClick={saveThreshold} disabled={savingThreshold}>
                {savingThreshold ? "Saving…" : "Apply"}
              </Button>
            )}
          </div>
          <p className="mt-2 text-xs text-muted">
            Decides how Discovery records a track you were offered. Playback is checked every 30 seconds, so this is
            approximate.
          </p>
        </Section>

        <Section title="Spotify API quota">
          {cooldown.active ? (
            <div className="text-sm">
              <div className="text-amber-300">Artist lookups paused for {cooldown.remaining}.</div>
              <p className="mt-1 text-xs text-muted">{cooldown.reason}</p>
              <Button
                variant="secondary"
                className="mt-3"
                title="Only if you know the quota has reset. If it has not, the next artist lookup will fail and re-arm the 24-hour pause."
                onClick={async () => {
                  try {
                    await api.clearQuotaCooldown();
                    await refreshQuota();
                    toast("info", "Cooldown cleared. If Spotify is still out of quota, it will re-arm on the next failure.");
                  } catch (e) {
                    toast("error", errorMessage(e));
                  }
                }}
              >
                Clear cooldown early
              </Button>
            </div>
          ) : (
            <p className="text-sm text-muted">
              No cooldown active. If Spotify reports its quota exhausted, artist and album lookups pause for 24 hours
              automatically; other features keep working.
            </p>
          )}
        </Section>

        <Section title="Storage">
          <Row label="Database" value={settings.dbPath} mono />
        </Section>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-line bg-panel p-5">
      <h2 className="mb-3 text-xs font-semibold uppercase tracking-wide text-muted">{title}</h2>
      {children}
    </section>
  );
}

function Row({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline gap-4 py-1 text-sm">
      <span className="w-28 shrink-0 text-muted">{label}</span>
      <span className={`select-text break-all ${mono ? "font-mono text-xs" : ""}`}>{value}</span>
    </div>
  );
}
