<p align="center">
  <img src="assets/icon.png" width="96" alt="fast.yt icon">
</p>

<h1 align="center">fast.yt</h1>

<p align="center">A fast, ad-free YouTube music player for the desktop.</p>

---

fast.yt is a small native app for listening to music from YouTube without the browser. It opens in a fraction of a second, searches in about half a second, and plays audio in the background with no ads, no feed, and no video decoding.

It is written in Rust with [egui](https://github.com/emilk/egui). Playback is handled by [mpv](https://mpv.io) and [yt-dlp](https://github.com/yt-dlp/yt-dlp), which you install separately.

## Features

- Search results with thumbnails, durations and view counts
- Audio-only playback with play/pause, seek, volume and skip
- Autoplay: when a track ends, the next search result starts
- Keyboard-first: search, navigate and play without the mouse
- Paste a YouTube link to play it directly
- Native window, low memory use, no Electron

## Install

You need [Rust](https://rustup.rs), plus mpv and yt-dlp on your machine. The scripts below build from source and install fast.yt so you can launch it like any other app.

### macOS

```bash
brew install mpv yt-dlp deno
git clone https://github.com/ChrisJamesHassell/fastyt.git
cd fastyt
./scripts/install-macos.sh
```

This creates `~/Applications/fast.yt.app`. Open it from Spotlight by typing **fast.yt**.

### Windows

```powershell
winget install shinchiro.mpv yt-dlp.yt-dlp DenoLand.Deno
git clone https://github.com/ChrisJamesHassell/fastyt.git
cd fastyt
.\scripts\install-windows.ps1
```

This installs to `%LOCALAPPDATA%\Programs\fast.yt` and adds **fast.yt** to the Start menu.

### Run without installing

```bash
cargo run --release
```

### Uninstall

Run `./scripts/uninstall-macos.sh` or `.\scripts\uninstall-windows.ps1`.

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| `/` or `Ctrl/Cmd+L` | Focus search |
| `Enter` | Search, or play the selected result |
| Arrow keys | Move between results |
| `Space` | Pause / resume |
| `Esc` | Leave the search box |

## How it works

- **Search** calls YouTube's web search endpoint directly over HTTPS and falls back to `yt-dlp` if that fails.
- **Playback** runs mpv in the background with video disabled. fast.yt controls it over mpv's [JSON IPC](https://mpv.io/manual/stable/#json-ipc) (a named pipe on Windows, a Unix socket on macOS) for pause, seek and volume, and to detect when a track ends.
- mpv uses yt-dlp to resolve the audio stream.

[deno](https://deno.com) is optional but recommended: yt-dlp uses it to answer YouTube's download challenges, and without it YouTube may throttle playback.

## Troubleshooting

**Search or playback stopped working.** Update yt-dlp first (`brew upgrade yt-dlp` or `winget upgrade yt-dlp.yt-dlp`). YouTube changes often and yt-dlp keeps up.

**Slow to start, buffering, or not working on a work network.** Run the built-in diagnostics. They check HTTPS, search, the stream lookup, download speed and a short silent playback, then print a report (also saved to your temp folder):

```bash
~/Applications/fast.yt.app/Contents/MacOS/fastyt --diagnose
```

```powershell
& "$env:LOCALAPPDATA\Programs\fast.yt\fastyt.exe" --diagnose | Out-Host
```

Add a YouTube link at the end to test a specific video.

**Corporate networks.** Some company networks (for example Cloudflare Zero Trust) inspect HTTPS traffic using a company certificate. fast.yt trusts your operating system's certificates, so it works as long as that certificate is installed system-wide, which IT normally does. Playback also recovers from dropped connections within about 10 seconds and shows **Buffering** while it waits for data.

## Development

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

The test suite is offline and deterministic. It never launches mpv, plays audio or touches the network. CI runs these checks on Windows and macOS for every push and pull request.

The app icon is drawn in code (`src/icon.rs`). The Windows icon is embedded at build time; after changing the icon, regenerate the committed macOS and README assets with:

```bash
cargo run --example icons
```

## Disclaimer

fast.yt is an independent project. It is not affiliated with, endorsed by, or sponsored by YouTube or Google. Use it in line with YouTube's Terms of Service.

## License

[MIT](LICENSE)
