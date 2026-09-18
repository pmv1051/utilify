import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage, type UpdateProgress } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { useQuotaCooldown } from "../lib/quota";

export function SettingsPage() {
  const settings = useApp((s) => s.settings);
  const refreshSettings = useApp((s) => s.refreshSettings);
  const refreshQuota = useApp((s) => s.refreshQuota);
  const setSetup = useApp((s) => s.setSetup);
  const toast = useApp((s) => s.toast);
  const cooldown = useQuotaCooldown();
  const availableUpdate = useApp((s) => s.availableUpdate);
  const setAvailableUpdate = useApp((s) => s.setAvailableUpdate);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [checkedOnce, setCheckedOnce] = useState(false);

  useEffect(() => {
    const un = listen<UpdateProgress>("update-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  async function checkUpdate() {
    setChecking(true);
    try {
      const u = await api.checkForUpdate();
      setAvailableUpdate(u);
      setCheckedOnce(true);
      if (!u) toast("info", "You are on the latest version.");
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setChecking(false);
    }
  }

  async function installUpdate() {
    setInstalling(true);
    setProgress(null);
    try {
      await api.installUpdate(); // restarts the app on success
    } catch (e) {
      toast("error", errorMessage(e));
      setInstalling(false);
    }
  }

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

        <Section title="Updates">
          <Row label="Version" value={settings.appVersion} mono />
          {availableUpdate ? (
            <div className="mt-2 text-sm">
              <div className="text-spotify">Utilify {availableUpdate.version} is available.</div>
              {availableUpdate.notes && (
                <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-ink p-3 text-xs text-zinc-300">{availableUpdate.notes}</pre>
              )}
              <div className="mt-3 flex items-center gap-3">
                <Button onClick={installUpdate} disabled={installing} title="Downloads the update, installs it and restarts Utilify">
                  {installing ? <Spinner /> : "Install and restart"}
                </Button>
                {installing && (
                  <span className="text-xs text-muted">
                    {progress
                      ? progress.total
                        ? `${Math.round((progress.downloaded / progress.total) * 100)}%`
                        : `${Math.round(progress.downloaded / 1024 / 1024)} MB`
                      : "Starting download…"}
                  </span>
                )}
              </div>
            </div>
          ) : (
            <div className="mt-2 flex items-center gap-3">
              <Button variant="secondary" onClick={checkUpdate} disabled={checking}>
                {checking ? <Spinner /> : "Check for updates"}
              </Button>
              <span className="text-xs text-muted">
                {checkedOnce ? "Up to date." : "Utilify checks once at launch. Updates come from GitHub Releases."}
              </span>
            </div>
          )}
        </Section>

        <Section title="Storage">
          <Row label="Database" value={settings.dbPath} mono />
          <p className="mt-2 text-xs text-muted">
            Logs: <span className="font-mono">%LOCALAPPDATA%\com.utilify.desktop\logs\Utilify.log</span> (Windows). Nothing
            leaves this computer except calls to Spotify's API.
          </p>
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
