# Utilify

Desktop utilities for Spotify playlists.

- **Randomizer** — a true Fisher-Yates shuffle, written into a shadow playlist (`Name-Utilify`) and played with Spotify shuffle off. Re-shuffles itself when you reach the end, picking up anything added to the source.
- **Bench** — take a song out of a playlist for an hour, a day, a month, or any time you choose. It comes back by itself, even if the app was closed.
- **Duplicates** — find the same track twice in one playlist or across several, including remasters and deluxe editions under different IDs, and remove the copies you pick.
- **Diff** — compare two playlists: only in A, only in B, in both. Add, remove, or spin a new playlist from any group.
- **Merge** — combine playlists into one, deduplicated, optionally shuffled.
- **Discography** — search an artist, tick the releases you want, get a playlist of their catalogue without duplicates across editions.
- **Editor** — move tracks to the top, bottom, or after another track, bulk move, sort by artist, album, title, date added or duration. Keeps "date added" intact.
- **Export / Import** — CSV or `Artist — Track` text out; paste a list in and match it back to Spotify with fuzzy matching you can correct.
- **Stats** — import Spotify's data export and see your whole listening history: time of day, weekday and
  monthly patterns, most played songs, artists and albums, what you skip past, and which devices you use.
- **Now Playing bar** — resizable; transport controls, seek, bench the playing track, re-shuffle the playing playlist.

Utilify runs entirely on your computer. Nothing is sent anywhere except Spotify's API.

## Requirements

- **Spotify Premium.** Spotify's API only allows playback control for Premium accounts, and its Development Mode requires it too.
- **Your own Spotify app** (free, about two minutes; the first launch walks you through it). Utilify ships without a Client ID so there is no shared user limit.
- Windows 10/11, macOS 11+, or a Linux desktop with WebKitGTK. **Only Windows is tested.** The macOS and Linux builds
  are published with every release but untested. Expect rough edges there, and please open an issue if something
  breaks.

## Install

These links always fetch the newest release. Utilify is developed and tested on Windows only; the macOS and Linux
downloads are untested builds.

| Platform | Download |
| --- | --- |
| Windows | [Utilify-windows-x64-setup.exe](https://github.com/pmv1051/utilify/releases/latest/download/Utilify-windows-x64-setup.exe) |
| macOS (Apple Silicon, M1 or newer) | [Utilify-macos-apple-silicon.dmg](https://github.com/pmv1051/utilify/releases/latest/download/Utilify-macos-apple-silicon.dmg) |
| macOS (Intel) | [Utilify-macos-intel.dmg](https://github.com/pmv1051/utilify/releases/latest/download/Utilify-macos-intel.dmg) |
| Linux | [Utilify-linux-x86_64.AppImage](https://github.com/pmv1051/utilify/releases/latest/download/Utilify-linux-x86_64.AppImage) (any distro) or [Utilify-linux-amd64.deb](https://github.com/pmv1051/utilify/releases/latest/download/Utilify-linux-amd64.deb) (Debian/Ubuntu) |

All versions are on the [Releases page](https://github.com/pmv1051/utilify/releases). Ignore the `.sig`, `.app.tar.gz` and `latest.json` files there: they belong to the in-app updater. "Source code" is added by GitHub automatically.

The installers are not code-signed with a paid certificate, so Windows SmartScreen and macOS Gatekeeper will warn on first launch. On Windows choose "More info → Run anyway"; on macOS right-click the app and choose "Open".

Updates are never installed automatically. **Settings → Updates** has a manual "Check for updates" button and an opt-in switch (off by default) that lets Utilify check GitHub Releases every 6 hours and notify you; installing is always your click.

## First launch

1. **Create a Spotify app.** Open the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard), log in, click **Create app**. Name and description can be anything.
2. **Add the redirect URI** exactly as shown in Utilify's setup screen: `http://127.0.0.1:8377/callback`. Tick **Web API**, accept the terms, save.
3. **Copy the Client ID** from the app's settings into Utilify. You do not need the client secret.
4. **Connect.** Your browser opens Spotify's consent page; approve, and it sends you back to Utilify. Tokens are stored only in Utilify's local database.
5. Pick a playlist and hit **Randomize**.

Closing the window keeps Utilify in the system tray so automatic re-shuffles and bench restores keep running. Quit from the tray menu to exit fully, or turn this off in Settings.

## Things to know

- **Playback is checked every 30 seconds** to stay within Spotify's rate limits. Re-shuffles and restores happen on the next check, so expect up to half a minute of delay.
- **Development Mode limits.** Spotify caps some listing endpoints and forbids others for apps like this one: liked-song lookups, batch artist lookups, artist genre tags, and reading playlists you don't own all fail. Utilify only offers what works.
- **API quota.** Heavy artist/album browsing (Discography) can exhaust your app's daily quota. When Spotify reports that, Utilify pauses those calls for 24 hours and shows a countdown; everything else keeps working.
- **Stats come from your data export**, not from the API: Spotify has no history endpoint. Request
  *Extended streaming history* at Account → Privacy settings → Download your data, then import the zip on
  the Stats page. It is stored in Utilify's local database, and the IP addresses in the export are not read.
  Importing the same export twice changes nothing, so a newer export just adds the plays since the last one.
- **The export has no playlist information**, so listening per playlist cannot be shown.

## Build from source

Prerequisites: [Node.js 20.19+](https://nodejs.org), [Rust stable](https://rustup.rs), and the [Tauri 2 system dependencies](https://tauri.app/start/prerequisites/) for your platform (on Windows: Visual Studio Build Tools with C++ and the WebView2 runtime, both usually present).

```bash
git clone https://github.com/pmv1051/utilify.git
cd utilify
npm install
npm run tauri dev        # run with hot reload
npm run tauri build      # produce installers in src-tauri/target/release/bundle
```

Checks: `npx tsc --noEmit` for the frontend, `cargo test` inside `src-tauri` for the backend.

### Layout

```
src/                 React + TypeScript frontend (Vite, Tailwind)
  pages/             one file per screen
  components/        NowPlaying bar, sidebar, pickers
  lib/api.ts         typed wrappers for every Tauri command
src-tauri/src/
  spotify/           auth (PKCE), HTTP client (rate limits, quota, paging), endpoint modules
  features/          randomizer, bench, duplicates, diff, merge, discography, editor, export/import, updater
  db/                SQLite schema (append-only migrations) and queries
  polling.rs         the single 30 s playback loop every feature subscribes to
```

## Releasing

1. Bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`.
2. Tag and push: `git tag v0.2.0 && git push --tags`.
3. The **Release** workflow builds Windows, macOS (both architectures) and Linux installers and attaches them, plus the updater's `latest.json`, to a **draft** release. Check the assets, then publish.

The updater verifies downloads with the public key in `tauri.conf.json`. The matching private key must be available to the workflow as the repository secrets `TAURI_SIGNING_PRIVATE_KEY` (file contents) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (empty if the key has none). Generate a pair with `npm run tauri signer generate -- -w ~/.tauri/utilify.key` and never commit the private key.

## Contributing

Issues and pull requests are welcome. Before opening a PR:

- run `npx tsc --noEmit` and `cargo test` (the CI workflow runs both);
- keep to the rules in the codebase: never replace a playlist that is playing (edit incrementally), never poll faster than every 30 seconds, batch playlist writes by 100, and don't add endpoints without checking they work in Development Mode;
- no Client IDs, tokens, or personal data in commits.

## License

MIT. See [LICENSE](LICENSE).
