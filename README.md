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

- Left click: smoothly expand the quota circle into the Weekly and token-history card.
- Click another window or press Escape: collapse back to the saved circle position.
- Drag the circle, or drag non-interactive card background: move and remember the position.
- Hover a heatmap cell: show its date and exact daily token count.
- Right click the circle: refresh or quit.
- Green/yellow/red indicate Weekly remaining quota; the heatmap uses relative logarithmic green levels for the latest 26 weeks.

## Troubleshooting

- “找不到 Codex CLI”: ensure `codex` is on the desktop session's `PATH`.
- “Codex 尚未登录”: run `codex login`, then choose Retry.
- “Token 历史不可用”: the installed Codex version may not expose `account/usage/read`; check `codex --version` and update Codex.
- Protocol incompatibility: run `codex --version` and update Codex Quota Ball or Codex CLI to a compatible version.

Codex app-server is an experimental local protocol. This application reads only `account/rateLimits/read` and `account/usage/read`, stores no credentials, prompts, source code, or local token database, and may require compatibility updates when Codex changes.

## Uninstall

```bash
./scripts/uninstall.sh
```

Uninstalling removes only the executable, application entry, autostart entry, and icon. It keeps `~/.config/codex-quota-ball/config.json` so the saved position survives reinstalling.
