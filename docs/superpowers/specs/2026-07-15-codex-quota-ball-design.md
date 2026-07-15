# Codex Quota Ball Design

## Goal

Build a small standalone desktop application for Ubuntu GNOME on X11 that stays above normal windows, shows the remaining Codex quota as a draggable floating ball, and expands on click to show both quota windows and their reset times.

## Scope

### Included in version 1

- Ubuntu GNOME on X11 only.
- A standalone Rust application using `eframe`/`egui`.
- A borderless, transparent, always-on-top compact window.
- A circular quota indicator showing the primary window's remaining percentage.
- A click-expanded card showing primary and secondary quota windows.
- Remaining percentage, reset time, refresh status, and stale-data status.
- Automatic refresh every 60 seconds and immediate refresh on expansion or manual request.
- Dragging and persistent window position.
- Launch from the desktop application menu.
- Automatic launch after the user logs in.
- Installation as one application executable plus desktop integration files.
- Clear states for missing Codex CLI, signed-out accounts, protocol errors, timeouts, and child-process exits.

### Excluded from version 1

- Wayland support.
- KDE and other Linux desktop environments.
- Debian package creation.
- API-key billing or OpenAI API organization usage.
- Reading, copying, or storing Codex credentials.
- Reset-credit consumption or any other account mutation.
- Detailed historical token-usage charts.
- Multiple profiles, themes, notification rules, and settings screens.
- A background daemon separate from the UI process.

## User Experience

### Compact state

The application normally displays an 88-pixel circular floating window. A progress ring and centered integer percentage show the remaining quota for the primary, short-duration Codex window. The entire visible circle is draggable.

- Left click without a drag expands the detail card.
- Drag moves the window and saves its final position.
- Right click opens a minimal context menu containing Refresh and Quit.
- The window remains above normal application windows.

The ring uses these thresholds:

- Green when remaining quota is at least 50 percent.
- Yellow when remaining quota is from 20 through 49 percent.
- Red when remaining quota is below 20 percent.
- Gray when no current quota data is available.

### Expanded state

The expanded card is anchored beside the compact ball and remains inside the current monitor's work area. It shows:

- A title and last-updated or stale-data label.
- Primary quota remaining percentage, progress bar, and reset time.
- Secondary quota remaining percentage, progress bar, and reset time when present.
- A manual refresh button and its in-progress state.
- A concise error message and Retry action when refreshing fails.

Clicking the ball again or clicking outside the application collapses the card. Expanding the card requests fresh data immediately unless a request is already running.

### Startup and persistence

The application starts in compact state at the last saved position. On the first run it appears near the upper-right corner of the primary monitor. If the saved position is outside the current monitor layout, it is clamped into the primary monitor's work area.

Only window position is persisted in `~/.config/codex-quota-ball/config.json`. The file is written after a completed drag, using a temporary file and rename so interruption cannot leave a partially written configuration.

## Architecture

The application is one OS process with a UI thread and one background worker. The Codex app-server runs as a child process only while needed by the application.

### `CodexClient`

`CodexClient` owns the `codex app-server --listen stdio://` child process, its standard input, and its standard output. It performs the app-server initialization handshake and then sends newline-delimited JSON-RPC requests. Quota refresh uses the read-only `account/rateLimits/read` method.

The locally installed Codex CLI 0.144.4 protocol schema exposes this method and returns a `RateLimitSnapshot` with optional `primary` and `secondary` windows. Each window contains `usedPercent`, `resetsAt`, and `windowDurationMins`. Because app-server is an experimental Codex surface, all response fields are treated as optional except the request identifier; unsupported or changed responses become a visible compatibility error rather than a panic.

`CodexClient` never opens Codex credential files. Authentication remains owned by the installed Codex CLI and its child app-server.

### `QuotaState`

`QuotaState` is the UI-facing snapshot. For each available rate-limit window it calculates:

```text
remaining_percent = clamp(100 - used_percent, 0, 100)
```

It also carries the last successful refresh time, whether a refresh is running, whether displayed data is stale, and the most recent user-facing error. Unix reset timestamps are formatted in the user's local timezone. A missing secondary window is displayed as unavailable and does not prevent the primary window from rendering.

### `FloatingApp`

`FloatingApp` owns the compact/expanded state, painting, hit testing, drag handling, monitor clamping, context-menu actions, and position persistence. It receives immutable `QuotaState` snapshots through a channel and sends refresh commands to the background worker through a second channel. It never blocks on child-process I/O.

### Background worker

The worker owns `CodexClient` and executes refresh commands serially. It requests quota at startup, every 60 seconds, when the card expands, and when the user selects Refresh or Retry. Duplicate commands are coalesced while one request is running.

Each request has a 10-second timeout. If the child process has exited or its pipes are unusable, the worker discards it and starts a new app-server on the next refresh attempt. No separate watchdog, exponential-backoff service, or persistent daemon is introduced.

## Data Flow

1. `FloatingApp` starts and loads the saved position.
2. The background worker launches `codex app-server --listen stdio://` and completes initialization.
3. The worker sends `account/rateLimits/read` with a unique request identifier.
4. `CodexClient` matches the response by identifier and ignores unrelated notifications.
5. The worker parses `primary` and `secondary`, converts used percentages to remaining percentages, and sends a new `QuotaState` snapshot to the UI.
6. The UI repaints the ball and expanded card without reading child-process streams itself.
7. On failure, the UI preserves the last successful snapshot, marks it stale, and presents the mapped error.

## Error Handling

The UI uses a gray ring with an exclamation mark when it has never loaded valid data. When valid data already exists, failures retain that data and display a stale indicator.

Errors map to specific messages and actions:

- `codex` not found: explain that Codex CLI must be installed and available on `PATH`; Retry rechecks `PATH`.
- Not logged in: explain that the user must run `codex login`; Retry starts a fresh app-server.
- Unsupported protocol or response shape: show the detected Codex version and explain that the application may need an update.
- Ten-second timeout: show that Codex did not respond; Retry restarts the child process.
- Child exit or broken pipe: show that the local Codex service stopped; the next refresh launches it again.
- Invalid reset timestamp: continue showing the percentage and label the reset time unavailable.
- Missing primary data: show quota unavailable rather than inventing a percentage.

The application does not log raw app-server responses, account identifiers, or credential data. Diagnostic logs contain only lifecycle events, Codex version, request duration, and categorized error names.

## Installation

The release workflow produces `target/release/codex-quota-ball`. The repository provides two user-scoped scripts:

- `scripts/install.sh` copies the executable to `~/.local/bin/codex-quota-ball`, installs the application entry at `~/.local/share/applications/codex-quota-ball.desktop`, installs the icon under `~/.local/share/icons/hicolor/scalable/apps/`, and creates `~/.config/autostart/codex-quota-ball.desktop`.
- `scripts/uninstall.sh` removes only those four installed artifacts and leaves the position configuration untouched so reinstalling preserves the user's placement.

Both desktop files launch `~/.local/bin/codex-quota-ball`. Installation is user-scoped and requires no `sudo`.

## Testing

### Unit tests

- Convert used percentages at 0, 50, 80, 100, and out-of-range values.
- Select green, yellow, red, and gray visual states at exact boundaries.
- Format future reset timestamps in local time and reject invalid timestamps safely.
- Parse responses with both windows, primary only, missing windows, unknown extra fields, and malformed required values.
- Load a valid saved position, fall back from malformed JSON, and clamp off-screen coordinates.

### Protocol tests

A small fake app-server executable used only by tests reads the same JSON-RPC stream and returns deterministic fixtures. Tests cover initialization, a successful quota response, unrelated notifications before the response, an error response, a timeout, malformed JSON, and process exit. These tests do not require a network connection or a logged-in Codex account.

### Manual acceptance on Ubuntu GNOME X11

- The transparent window has no rectangular background and remains above normal windows.
- Clicking and dragging are distinguished reliably.
- Expansion, outside-click collapse, context-menu refresh, and quit work.
- The expanded card remains visible at every screen edge.
- Position survives application restart and is corrected after monitor-layout changes.
- Temporary Codex failure produces stale or unavailable state without freezing the UI.
- The desktop menu entry launches the application.
- A new login session automatically launches one application instance.
- The uninstall script removes installed integration files without deleting saved position.

## Success Criteria

Version 1 is complete when a user logged into Codex on Ubuntu GNOME X11 can run the install script, see the primary remaining quota in an always-on-top draggable ball, expand it to inspect both quota windows and reset times, recover visibly from Codex failures, retain the position across restarts, and receive the application automatically after the next desktop login.
