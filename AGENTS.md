# Notification Center

Applications: apps/notifyd (the history service; no GTK) and apps/notification-center
(GTK, layer-shell panel). The store is crates/nyx-notify-store (SQLite schema, Omarchy
import, retention, pins, search; no GTK), with the unit tests.

Two modes (`nyx-notifyd --mode`; switched with `nyx-notification-center
--use-nyx-popups | --use-omarchy-popups`, apps/notification-center/src/switch.rs):
- Omarchy (stage A, default): Omarchy's shell owns `org.freedesktop.Notifications`;
  nyx-notifyd only reads `~/.local/state/omarchy/notifications/{live,history}.json`.
- Nyx (stage B): nyx-notifyd serves `org.freedesktop.Notifications` (apps/notifyd/src/fdo.rs),
  the panel process draws the pop-ups (popups.rs) from its `Popup`/`PopupClosed` signals,
  Omarchy's `omarchy.notifications` is in `disabledPlugins`, and the bridge plugin
  (shell-plugin/, id `design-nexus.notifications`) answers Omarchy's `notifications` IPC.
Do not disturb is `quiet.json` in both (written by nyx-notifyd in Nyx mode, keeping
Omarchy's other fields). Clicks and buttons always go through nyx-notifyd (`Activate`,
`InvokeAction`, `RunAction`). `cargo test -p nyx-notifyd --test compat` is the
compatibility suite (private dbus-daemon, real notify-send and omarchy-notification-send).

Rules: read, dismissed and pinned are separate flags; nothing but an explicit delete
removes a pinned row. A deleted row leaves a marker in `gone` so the next import doesn't
bring it back. The hidden panel listens to nothing.

Commands: `cargo test --locked --workspace`; `cargo clippy --workspace --all-targets`;
`./install.sh`. Try the panel on a headless output without focus:
`NYX_NC_NO_KEYBOARD=1 nyx-notification-center --only HEADLESS-<n> --show`
(`NYX_NC_TEST_VIEW=pinned|history|settings` opens a view). Try nyx-notifyd against copies
of the files with `XDG_STATE_HOME`/`XDG_DATA_HOME` pointing at a scratch folder (and
`dbus-run-session` for a private bus). Never inject input or steal focus.

Use the public, pinned shared crates; never import another application's internals.
Use canonical STYLE.md from the pinned nyx-design dependency.
Common guidance: https://github.com/design-nexus/nyx/blob/main/docs/AGENTS_COMMON.md
