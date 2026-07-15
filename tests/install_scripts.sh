#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
test_root="$(mktemp -d)"
test_home="$test_root/home with spaces & bars|slash\\path\"quoted"
mkdir -p "$test_home"
trap 'rm -rf "$test_root"' EXIT
fake_binary="$test_home/fake-codex-quota-ball"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'printf "%s\n" "$#" "$0" > "$HOME/launch-result"' \
  > "$fake_binary"
chmod +x "$fake_binary"
HOME="$test_home" "$repo/scripts/install.sh" "$fake_binary"
test -x "$test_home/.local/bin/codex-quota-ball"
test -f "$test_home/.local/share/applications/codex-quota-ball.desktop"
test -f "$test_home/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg"
test -f "$test_home/.config/autostart/codex-quota-ball.desktop"
desktop_file="$test_home/.local/share/applications/codex-quota-ball.desktop"
autostart_file="$test_home/.config/autostart/codex-quota-ball.desktop"
/usr/bin/desktop-file-validate --no-hints "$desktop_file" "$autostart_file"
HOME="$test_home" /usr/bin/gio launch "$desktop_file"
for _ in {1..20}; do
  [[ -f "$test_home/launch-result" ]] && break
  sleep 0.05
done
mapfile -t launch_result < "$test_home/launch-result"
test "${launch_result[0]}" = 0
test "${launch_result[1]}" = "$test_home/.local/bin/codex-quota-ball"
mkdir -p "$test_home/.config/codex-quota-ball"
printf '%s\n' '{"x":1,"y":2}' > "$test_home/.config/codex-quota-ball/config.json"
HOME="$test_home" "$repo/scripts/uninstall.sh"
test ! -e "$test_home/.local/bin/codex-quota-ball"
test ! -e "$test_home/.local/share/applications/codex-quota-ball.desktop"
test ! -e "$test_home/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg"
test ! -e "$test_home/.config/autostart/codex-quota-ball.desktop"
test -f "$test_home/.config/codex-quota-ball/config.json"
