# Notification Center

Applications: apps/notifyd (the history service; no GTK) and apps/notification-center
(GTK, layer-shell panel). The store is crates/nyx-notify-store (SQLite schema, Omarchy
import, retention, pins, search; no GTK), with the unit tests.

Stage A (now): Omarchy's shell owns `org.freedesktop.Notifications` and shows the
pop-ups; nyx-notifyd only reads `~/.local/state/omarchy/notifications/{live,history,quiet}.json`
and never writes there. Do not disturb is changed only through Omarchy's own commands.
Stage B (replacing the daemon) waits for a compatibility suite; see
nyx/docs/desktop/ARCHITECTURE.md §4.4.

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
