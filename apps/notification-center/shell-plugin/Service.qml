import QtQuick
import Quickshell
import Quickshell.Io

// Omarchy's shortcuts (Super + comma and friends) and scripts
// (omarchy-toggle-notification-silencing, -dismiss, -wait) talk to the shell's
// `notifications` IPC. While Nyx Notification Center receives notifications, Omarchy's
// own notification service is off, so this answers in its place and passes each call
// to nyx-notifyd. Only one of the two is ever on.
Item {
  id: root

  property var shell: null
  property var manifest: null

  readonly property string stateHome: Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")
  property bool dnd: false
  property int inbox: 0

  function call(method, signature, args) {
    var cmd = ["busctl", "--user", "call", "io.github.design_nexus.nyx.Notifications",
      "/io/github/design_nexus/nyx/Notifications", "io.github.design_nexus.nyx.Notifications1", method]
    if (signature) cmd = cmd.concat([signature]).concat(args || [])
    Quickshell.execDetached(cmd)
  }

  function open(view) {
    Quickshell.execDetached(view ? ["nyx-notification-center", "--toggle", view] : ["nyx-notification-center", "--toggle"])
  }

  function truthy(v) {
    var s = String(v).toLowerCase()
    return s === "on" || s === "true" || s === "1" || s === "yes"
  }

  IpcHandler {
    target: "notifications"

    function ping(): string { return "ok" }
    function isDnd(): string { return root.dnd ? "on" : "off" }
    function toggleDnd(): string {
      root.dnd = !root.dnd
      root.call("SetDnd", "b", [root.dnd ? "true" : "false"])
      return root.dnd ? "on" : "off"
    }
    function setDnd(value: string): string {
      root.dnd = root.truthy(value)
      root.call("SetDnd", "b", [root.dnd ? "true" : "false"])
      return root.dnd ? "on" : "off"
    }
    function act(): string { root.call("ActNewest"); return "ok" }
    function dismissOne(): string { root.call("DismissNewest"); return "ok" }
    function dismissAll(): string { root.call("DismissPopups"); return "ok" }
    function invokeLast(): string { root.call("ActNewest"); return "ok" }
    function invokeAction(n: int): string { root.call("ActionNewest", "u", [String(n)]); return "ok" }
    function keyboard(): string { root.open(""); return "ok" }
    function showHistory(): string { root.open("history"); return "ok" }
    function dismiss(summary: string): string { root.call("DismissMatching", "s", [summary]); return "ok" }
    function count(): string { return String(root.inbox) }
    function expand(): string { return "ok" }
    function probe(): string { return JSON.stringify({ server: "nyx-notifyd", dnd: root.dnd, inbox: root.inbox }) }
  }

  FileView {
    path: root.stateHome + "/omarchy/notifications/quiet.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try { root.dnd = JSON.parse(text()).dnd === true } catch (e) {}
    }
  }

  FileView {
    path: root.stateHome + "/nyx/notifications.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try { root.inbox = JSON.parse(text()).inbox || 0 } catch (e) {}
    }
  }
}
