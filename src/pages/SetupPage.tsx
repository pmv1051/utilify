import { useState } from "react";
import { api, errorMessage } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";

const DASHBOARD_URL = "https://developer.spotify.com/dashboard";

export function SetupPage() {
  const setup = useApp((s) => s.setup)!;
  const setSetup = useApp((s) => s.setSetup);
  const loadAll = useApp((s) => s.loadAll);
  const toast = useApp((s) => s.toast);

  const [clientId, setClientId] = useState("");
  const [saving, setSaving] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const step = setup.hasClientId ? 2 : 1;

  async function saveClientId() {
    const trimmed = clientId.trim();
    if (!/^[0-9a-f]{32}$/i.test(trimmed)) {
      setError("A Spotify Client ID is 32 hexadecimal characters. Double-check what you pasted.");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await api.saveClientId(trimmed);
      setSetup(await api.getSetupState());
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  async function connect() {
    setConnecting(true);
    setError(null);
    try {
      const next = await api.startAuth();
      setSetup(next);
      toast("success", `Connected as ${next.userDisplayName ?? "Spotify user"}.`);
      await loadAll();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setConnecting(false);
    }
  }

  async function changeClientId() {
    try {
      await api.disconnect();
      await api.saveClientId("");
      setSetup(await api.getSetupState());
    } catch (e) {
      setError(errorMessage(e));
    }
  }

  return (
    <div className="flex h-full items-center justify-center overflow-auto p-8">
      <div className="w-full max-w-2xl">
        <div className="mb-8 flex items-center gap-3">
          <span className="flex h-10 w-10 items-center justify-center rounded-full bg-spotify text-xl font-black text-black">
            U
          </span>
          <div>
            <h1 className="text-2xl font-bold">Welcome to Utilify</h1>
            <p className="text-sm text-muted">Spotify playlist tools that Spotify forgot to build.</p>
          </div>
        </div>

        <ol className="mb-6 flex gap-6 text-sm">
          <StepLabel n={1} active={step === 1} done={step > 1} label="Spotify app" />
          <StepLabel n={2} active={step === 2} done={false} label="Connect account" />
        </ol>

        {step === 1 ? (
          <section className="rounded-lg border border-line bg-panel p-6">
            <h2 className="mb-2 text-lg font-semibold">Create your own Spotify app (about 2 minutes)</h2>
            <p className="mb-4 text-sm text-muted">
              Utilify does not ship with a shared Spotify Client ID. Every user registers a free developer app so
              there is no user cap and no waitlist. You need Spotify Premium for playback control.
            </p>
            <ol className="mb-5 list-decimal space-y-2 pl-5 text-sm text-zinc-300">
              <li>
                Open the{" "}
                <button className="text-spotify underline" onClick={() => api.openExternal(DASHBOARD_URL)}>
                  Spotify Developer Dashboard
                </button>{" "}
                and log in with your Spotify account.
              </li>
              <li>
                Click <b>Create app</b>. Any name and description work (for example, "Utilify").
              </li>
              <li>
                Under <b>Redirect URIs</b>, add exactly: <Code>{setup.redirectUri}</Code>
              </li>
              <li>
                Under <b>Which API/SDKs are you planning to use?</b> tick <b>Web API</b>, accept the terms and save.
              </li>
              <li>
                Open the app's <b>Settings</b> and copy the <b>Client ID</b>. You do not need the client secret.
              </li>
            </ol>
            <label className="mb-1 block text-xs font-medium uppercase tracking-wide text-muted">Client ID</label>
            <div className="flex gap-2">
              <input
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && saveClientId()}
                placeholder="32-character Client ID"
                spellCheck={false}
                className="flex-1 rounded-md border border-line bg-ink px-3 py-2 font-mono text-sm outline-none focus:border-spotify"
              />
              <Button local onClick={saveClientId} disabled={saving || clientId.trim().length === 0}>
                {saving && <Spinner />} Continue
              </Button>
            </div>
            {error && <p className="mt-3 text-sm text-red-400">{error}</p>}
          </section>
        ) : (
          <section className="rounded-lg border border-line bg-panel p-6">
            <h2 className="mb-2 text-lg font-semibold">Connect your Spotify account</h2>
            <p className="mb-4 text-sm text-muted">
              Your browser will open Spotify's consent page. After you approve, Spotify sends you back to Utilify
              on <Code>{setup.redirectUri}</Code>. Tokens are stored only in the local database on this computer.
            </p>
            <div className="flex items-center gap-3">
              <Button onClick={connect} disabled={connecting}>
                {connecting ? (
                  <>
                    <Spinner /> Waiting for Spotify…
                  </>
                ) : (
                  "Connect to Spotify"
                )}
              </Button>
              <Button local variant="ghost" onClick={changeClientId} disabled={connecting}>
                Use a different Client ID
              </Button>
            </div>
            {connecting && (
              <p className="mt-3 text-xs text-muted">
                If the browser did not open, check your default browser. This request expires after 5 minutes.
              </p>
            )}
            {error && <p className="mt-3 text-sm text-red-400">{error}</p>}
          </section>
        )}
      </div>
    </div>
  );
}

function StepLabel({ n, active, done, label }: { n: number; active: boolean; done: boolean; label: string }) {
  return (
    <li className={`flex items-center gap-2 ${active ? "text-white" : "text-muted"}`}>
      <span
        className={`flex h-6 w-6 items-center justify-center rounded-full text-xs font-bold ${
          done ? "bg-spotify text-black" : active ? "bg-white text-black" : "bg-panel-2"
        }`}
      >
        {done ? "✓" : n}
      </span>
      {label}
    </li>
  );
}

function Code({ children }: { children: string }) {
  return (
    <code
      className="cursor-pointer select-text rounded bg-ink px-1.5 py-0.5 font-mono text-xs text-spotify"
      title="Click to copy"
      onClick={() => navigator.clipboard?.writeText(children)}
    >
      {children}
    </code>
  );
}
