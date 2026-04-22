![today](assets/public/스크린샷%202026-04-22%20130103.png)

A Windows desktop app that tracks active window usage time and stores it locally. Built with Rust and egui — single executable, no external service.

## Features

- Real-time active window tracking via `wineventhook` + 1-second polling fallback
- Idle detection — pauses tracking when input stops
- Today view: bar chart of per-app usage time
- History view: daily / monthly breakdown
- App manager: hide apps from the display list (tracking continues silently)
- System tray with show/hide toggle
- Auto-launch at Windows startup (toggle in UI)
- Single-instance guard
- SQLite storage in local app data

## Screenshots

| Today | History | App Manager |
|-------|---------|-------------|
| ![today](assets/public/스크린샷%202026-04-22%20130103.png) | ![history](assets/public/스크린샷%202026-04-22%20130111.png) | ![app manager](assets/public/스크린샷%202026-04-22%20130115.png) |

## Download

| File | Platform |
|------|----------|
| [chronograph.exe](https://github.com/qmdch1/chronograph/releases/latest/download/chronograph.exe) | Windows (x64) |

## Commands

| Command | Description |
|---------|-------------|
| `cargo run` | Run in dev mode |
| `cargo build --release` | Build release exe |

## Tech Stack

| | |
|---|---|
| Language | Rust (edition 2024) |
| GUI | egui 0.34 + eframe |
| Window tracking | `wineventhook` (WinEvent hook) + `x-win` |
| Idle detection | `windows` crate — `GetLastInputInfo` |
| Storage | SQLite via `sqlx` with async migrations |
| Tray | `tray-icon` |
| Auto-launch | `auto-launch` |
| Release optimization | `opt-level = 3`, thin LTO, strip |

## How It Works

1. **WinEvent hook** — catches `SYSTEM_FOREGROUND` events instantly when focus changes
2. **Polling fallback** — 1-second tick catches windows that don't fire hooks (Electron, VS Code, etc.)
3. **Idle guard** — `GetLastInputInfo` pauses logging when the user is away
4. **Aggregator** — merges hook and idle events, writes sessions to SQLite
5. **App manager** — hidden apps are excluded from the UI but still recorded in the DB
