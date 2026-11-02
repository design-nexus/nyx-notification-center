#!/usr/bin/env bash
# Remove Nyx Notification Center.
#
#   ./uninstall.sh            remove the programs and their services
#   ./uninstall.sh --purge    also delete the notification history and settings
#                             (~/.local/share/nyx/notifications.db)
set -euo pipefail
say() { printf '\033[1m==>\033[0m %s\n' "$*"; }

bin="$HOME/.local/bin"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
state="${XDG_STATE_HOME:-$HOME/.local/state}"
systemctl --user disable --now nyx-notification-center.service nyx-notifyd.service 2>/dev/null || true
rm -f "$bin/nyx-notification-center" "$bin/nyx-notifyd" \
  "$config/systemd/user/nyx-notification-center.service" "$config/systemd/user/nyx-notifyd.service" \
  "$share/dbus-1/services/io.github.design_nexus.nyx.Notifications.service" "$state/nyx/notifications.json"
omarchy-plugin-disable design-nexus.notification-center >/dev/null 2>&1 || true
rm -rf "$config/omarchy/plugins/design-nexus.notification-center"
systemctl --user daemon-reload 2>/dev/null || true
if [[ ${1:-} == --purge ]]; then
  rm -f "$share/nyx/notifications.db" "$share/nyx/notifications.db-wal" "$share/nyx/notifications.db-shm"
fi
say "Nyx Notification Center removed. Omarchy's own notifications are unchanged."
