import { Component, type ErrorInfo, type ReactNode } from "react";

interface State {
  error: Error | null;
}

/** Last line of defence: a render crash shows a message instead of a blank window. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Utilify UI crashed:", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="max-w-lg rounded-lg border border-red-500/40 bg-panel p-6">
          <h1 className="text-lg font-semibold">Something went wrong in the interface</h1>
          <p className="mt-2 text-sm text-muted">
            The background work (re-shuffles, bench restores) keeps running. Reload the window to continue; if this
            keeps happening, the log at <span className="font-mono text-xs">%LOCALAPPDATA%\com.utilify.desktop\logs</span>{" "}
            has the details.
          </p>
          <pre className="mt-3 max-h-40 overflow-auto rounded bg-ink p-3 text-xs text-red-300">{String(this.state.error)}</pre>
          <button
            onClick={() => window.location.reload()}
            className="mt-4 rounded-md bg-spotify px-3.5 py-2 text-sm font-semibold text-black hover:bg-spotify-dark"
          >
            Reload
          </button>
        </div>
      </div>
    );
  }
}
