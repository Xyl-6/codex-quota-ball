# Codex Quota Ball

A small always-on-top quota indicator for Codex on Ubuntu GNOME X11.

## Requirements

- Ubuntu GNOME with `XDG_SESSION_TYPE=x11`
- A working `codex` command logged in through ChatGPT
- Rust and Cargo when installing from source

## Install

From the source checkout:

```bash
./scripts/install.sh
```

The script builds a release binary when needed, installs it under `~/.local`, adds an application-menu entry, and enables login startup. It installs only for the current user and does not use `sudo`.

To install an existing executable instead of building from source:

```bash
./scripts/install.sh /absolute/path/to/codex-quota-ball
```

## Use

- Left click: expand or collapse quota details.
- Drag: move and remember the ball position.
- Right click: refresh or quit.
- Green means at least 50% remains, yellow means 20–49%, red means below 20%, and gray means quota is unavailable.

## Troubleshooting

- “找不到 Codex CLI”: ensure `codex` is on the desktop session's `PATH`.
- “Codex 尚未登录”: run `codex login`, then choose Retry.
- Protocol incompatibility: run `codex --version` and update Codex Quota Ball or Codex CLI to a compatible version.

Codex app-server is an experimental local protocol. This application reads only `account/rateLimits/read`, stores no credentials, and may require compatibility updates when Codex changes.

## Uninstall

```bash
./scripts/uninstall.sh
```

Uninstalling removes only the executable, application entry, autostart entry, and icon. It keeps `~/.config/codex-quota-ball/config.json` so the saved position survives reinstalling.
