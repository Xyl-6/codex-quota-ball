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

exec_value="${destination//\\/\\\\\\\\}"
exec_value="${exec_value//\"/\\\"}"
render_desktop_file() {
  local template="$1"
  local output="$2"
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == 'Exec="@EXEC@"' ]]; then
      printf 'Exec="%s"\n' "$exec_value"
    else
      printf '%s\n' "$line"
    fi
  done < "$template" > "$output"
}
render_desktop_file "$root/packaging/codex-quota-ball.desktop.in" "$HOME/.local/share/applications/codex-quota-ball.desktop"
render_desktop_file "$root/packaging/codex-quota-ball-autostart.desktop.in" "$HOME/.config/autostart/codex-quota-ball.desktop"
printf 'Installed Codex Quota Ball for the current user.\n'
