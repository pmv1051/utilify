import { useState } from "react";
import { api, errorMessage } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";

export function SettingsPage() {
  const settings = useApp((s) => s.settings);
  const refreshSettings = useApp((s) => s.refreshSettings);
  const setSetup = useApp((s) => s.setSetup);
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);

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
                Required for automatic re-shuffles and (later) bench restores while you are not looking at the app.
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
