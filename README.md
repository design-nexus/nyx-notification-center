# Notification Center

Notifications for [Omarchy](https://omarchy.org) and Hyprland that stay around: a panel on
the right edge of the screen with an Inbox, pinned notifications and searchable history.
It takes its colors from your Omarchy theme.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-notification-center/main/install.sh | bash
```

It runs as two user services and stays hidden until you open it:

```sh
nyx-notification-center --toggle           # open or close (bind a key to this)
nyx-notification-center --toggle history   # open on a view (inbox, pinned, history)
```

Click outside it or press Esc to close it. A bar button is installed too (a bell, with a
dot for unread notifications); add it from Settings → Plugins (or
`omarchy-plugin-enable design-nexus.notification-center right`). Right-click it to switch
Do not disturb.

## What it does

- **Inbox**: notifications you haven't dealt with, grouped by app (click a group to open
  it). A pop-up that timed out lands here; one you clicked or closed, or that its app
  withdrew, goes straight to history. Hover a notification to pin or dismiss it; click it
  to run what the sender asked for, or open its app. Clear All empties the Inbox.
- **Pinned**: kept, with their text, until you unpin or delete them: dismissing, Clear
  All, history retention and deleting history leave them alone.
- **History**: everything, newest first and by day. Search by title, text or app, and
  filter by app and by date.
- **Settings** (the gear): Do not disturb, also on a schedule; how long history is kept
  (7, 30 or 90 days); delete history; which apps are kept in history.
- Opening it closes the Nyx Control Center, and the other way round.

By default Omarchy still receives notifications and shows the pop-ups; `nyx-notifyd`
watches Omarchy's notification files and keeps a copy of each one in
`~/.local/share/nyx/notifications.db`, so history goes back further than Omarchy's own
last day. Buttons an app put on a pop-up then work only while the pop-up is showing.

To have Nyx receive notifications and show the pop-ups itself:

```sh
nyx-notification-center --use-nyx-popups      # and back: --use-omarchy-popups
nyx-notification-center --popups              # which is in use
```

Then `nyx-notifyd` is the desktop's notification service (`org.freedesktop.Notifications`)
and the pop-ups appear in the top-right corner, newest on top; the pointer on one holds
it. A pop-up that times out leaves its notification open in the Inbox, so the app's own
buttons (Reply, Mark as read…) keep working from there. Verification codes get a copy
button. Settings adds apps that arrive without a pop-up and pop-ups without their text.
Omarchy's notification shortcuts (Super + comma and the rest) and scripts keep working
through a small shell plugin, and Do not disturb stays Omarchy's own setting, so the bar
and `omarchy-toggle-notification-silencing` stay in step either way.

## Build

```sh
cargo build --release --locked -p nyx-notification-center -p nyx-notifyd
cargo test --locked --workspace
```

`nyx-notifyd` serves the history on the session bus as
`io.github.design_nexus.nyx.Notifications1` (`Inbox`, `Pinned`, `History`, `Apps`,
`Settings`, and the changes; JSON in and out; a `Changed` signal).
`cargo test -p nyx-notifyd --test compat` runs the compatibility suite: `nyx-notifyd` as
the notification service on a private bus, against the spec's calls, `notify-send` and
Omarchy's `omarchy-notification-send`.
`nyx-notification-center --only HEADLESS-1 --show` opens the panel on one output only.

## Remove

```sh
./uninstall.sh            # or --purge to delete the history too
```

## License

MIT
