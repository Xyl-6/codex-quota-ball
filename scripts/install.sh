#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$root/target/release/codex-quota-ball}"

if [[ $# -eq 0 && ! -x "$binary" ]]; then
  cargo build --release --manifest-path "$root/Cargo.toml"
fi
if [[ ! -x "$binary" ]]; then
  printf 'Executable not found: %s\n' "$binary" >&2
  exit 1
fi

destination="$HOME/.local/bin/codex-quota-ball"
install -Dm755 "$binary" "$destination"
install -Dm644 "$root/assets/codex-quota-ball.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg"
install -d "$HOME/.local/share/applications" "$HOME/.config/autostart"

replacement="${destination//\\/\\\\}"
replacement="${replacement//&/\\&}"
replacement="${replacement//|/\\|}"
sed "s|@EXEC@|$replacement|g" "$root/packaging/codex-quota-ball.desktop.in" > "$HOME/.local/share/applications/codex-quota-ball.desktop"
sed "s|@EXEC@|$replacement|g" "$root/packaging/codex-quota-ball-autostart.desktop.in" > "$HOME/.config/autostart/codex-quota-ball.desktop"
printf 'Installed Codex Quota Ball for the current user.\n'
