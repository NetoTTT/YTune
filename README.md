<h1 align="center">
  <img src="src-tauri/icons/icon.png" width="128" height="128" /><br>
  YTune
</h1>

<p align="center">
  YouTube Music desktop client with Discord Rich Presence and system tray controls.<br>
  Built with Tauri 2 and Svelte.
</p>

## Features

- Loads YouTube Music in a native window — no Electron, significantly lower memory usage
- System tray icon: click to open a mini control popup without switching to the full window
- Mini popup shows current song, progress bar (seekable), and playback controls
- Playlist queue visible directly in the popup when a playlist is playing
- Discord Rich Presence updates automatically with the current track
- Minimizes to tray instead of closing
- Single instance enforcement — reopening the app brings the existing window to focus
- Works on Windows and Linux

## Download

Installers are available on the [Releases](https://github.com/NetoTTT/YTune/releases) page.

## Building from source

### Requirements

- [Rust](https://rustup.rs/) (1.70+)
- [Node.js](https://nodejs.org/) (18+)
- On Linux: `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`

### Steps

```bash
git clone https://github.com/NetoTTT/YTune.git
cd YTune
npm install
npm run tauri build
```

The installer will be at `src-tauri/target/release/bundle/`.

For development with hot-reload:

```bash
npm run tauri dev
```

## Discord Rich Presence

To enable Discord Rich Presence, create an application at the [Discord Developer Portal](https://discord.com/developers/applications), copy the Client ID, and replace `YOUR_CLIENT_ID_HERE` in `src-tauri/src/lib.rs`.

## Tech stack

| Layer | Technology |
|-------|-----------|
| Shell | Tauri 2 (Rust) |
| Frontend | Svelte 5 + SvelteKit |
| Bundler | Vite |
| Discord RPC | discord-rich-presence |

## Credits

Developed with assistance from [Claude](https://claude.ai) (Anthropic).
