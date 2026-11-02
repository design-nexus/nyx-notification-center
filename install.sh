#!/usr/bin/env bash
# Install Nyx Notification Center.
#
#   curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-notification-center/main/install.sh | bash
#
# Two user services: nyx-notifyd keeps every notification in Nyx's history, and the
# panel opens with `nyx-notification-center --toggle` (bind a key to it, or use the bar
# button).
set -euo pipefail

REPO="design-nexus/nyx-notification-center"
say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[31m==>\033[0m %s\n' "$*" >&2; exit 1; }

command -v hyprctl >/dev/null || warn "Hyprland wasn't found. Nyx Notification Center is made for Omarchy (Hyprland)."

if command -v pacman >/dev/null; then
  need=()
  for p in gtk4 gtk4-layer-shell pkgconf gcc git; do pacman -Qq "$p" >/dev/null 2>&1 || need+=("$p"); done
  if ((${#need[@]})); then
    say "Installing ${need[*]}"
    sudo pacman -S --needed --noconfirm "${need[@]}"
  fi
fi

# This checkout, or a fresh one when piped from curl.
here=""
[[ -n ${BASH_SOURCE[0]:-} && -f ${BASH_SOURCE[0]} ]] && here=$(cd "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [[ -n $here && -f $here/Cargo.toml && -d $here/apps/notification-center ]]; then
  src=$here
else
  say "Fetching the source"
  git clone -q --depth 1 "https://github.com/$REPO.git" "$work/src"
  src="$work/src"
fi

if ! command -v cargo >/dev/null; then
  command -v pacman >/dev/null || die "Rust (cargo) is needed to build Nyx Notification Center. Install it and run this again."
  say "Installing rust"
  sudo pacman -S --needed --noconfirm rust
fi
say "Building Nyx Notification Center (a few minutes the first time)"
(cd "$src" && cargo build --release --locked -p nyx-notification-center -p nyx-notifyd)

bin="$HOME/.local/bin"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
units="$config/systemd/user"
dbus="${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services"
mkdir -p "$bin" "$units" "$dbus"
target="${CARGO_TARGET_DIR:-$src/target}/release"
data="$src/apps/notification-center/data"
# Replace the files rather than writing into them, so running copies keep their old
# binary until they restart.
for b in nyx-notification-center nyx-notifyd; do
  install -m 755 "$target/$b" "$bin/$b.new" && mv -f "$bin/$b.new" "$bin/$b"
done
install -m 644 "$data/nyx-notification-center.service" "$data/nyx-notifyd.service" "$units/"
install -m 644 "$data/io.github.design_nexus.nyx.Notifications.service" "$dbus/"
systemctl --user daemon-reload 2>/dev/null || true

# The bar button, for omarchy-shell. It's put in place, not added to the bar: turn it on
# from Settings → Plugins (or `omarchy-plugin-enable design-nexus.notification-center right`).
plugins="$config/omarchy/plugins"
if [[ -d $plugins ]]; then
  mkdir -p "$plugins/design-nexus.notification-center"
  install -m 644 "$src/apps/notification-center/plugin/manifest.json" "$src/apps/notification-center/plugin/BarWidget.qml" \
    "$plugins/design-nexus.notification-center/"
  omarchy-shell shell rescanPlugins >/dev/null 2>&1 || true
fi

# The history service has no window, so it simply restarts on the new build.
systemctl --user enable nyx-notifyd.service 2>/dev/null || true
systemctl --user restart nyx-notifyd.service 2>/dev/null || true
if systemctl --user is-active --quiet nyx-notification-center.service 2>/dev/null; then
  say "Installed. The running panel uses the new build after: systemctl --user restart nyx-notification-center"
else
  systemctl --user enable --now nyx-notification-center.service 2>/dev/null || true
  say "Installed. Open it with: nyx-notification-center --toggle"
fi
case ":$PATH:" in
  *":$bin:"*) ;;
  *) warn "$bin isn't on your PATH." ;;
esac
