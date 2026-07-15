#!/usr/bin/env bash
set -euo pipefail

rm -f \
  "$HOME/.local/bin/codex-quota-ball" \
  "$HOME/.local/share/applications/codex-quota-ball.desktop" \
  "$HOME/.local/share/icons/hicolor/scalable/apps/codex-quota-ball.svg" \
  "$HOME/.config/autostart/codex-quota-ball.desktop"
printf 'Uninstalled Codex Quota Ball; saved position was retained.\n'
