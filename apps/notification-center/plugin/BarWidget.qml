import QtQuick
import Quickshell
import Quickshell.Io
import qs.Ui

// The Notification Center's bar button: a bell, with a dot while the Inbox has unread
// notifications and crossed out during Do not disturb. It watches two small files
// (nyx-notifyd's count and Omarchy's quiet.json); nothing polls.
BarWidget {
  id: root
  moduleName: "design-nexus.notification-center"

  readonly property string stateHome: Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")
  property int unread: 0
  property int inbox: 0
  property bool dnd: false

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    // bell-off · bell-badge · bell
    text: root.dnd ? "\u{f009b}" : (root.unread > 0 ? "\u{f116b}" : "\u{f009a}")
    tooltipText: (root.dnd ? "Do not disturb · " : "")
      + (root.unread > 0 ? root.unread + " unread" : (root.inbox > 0 ? root.inbox + " in the Inbox" : "Notifications"))
    onPressed: function(b) {
      if (!root.bar) return
      if (b === Qt.RightButton) root.bar.run("omarchy-toggle-notification-silencing")
      else root.bar.run("nyx-notification-center --toggle")
    }
  }

  FileView {
    path: root.stateHome + "/nyx/notifications.json"
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      try {
        var v = JSON.parse(text())
        root.unread = v.unread || 0
        root.inbox = v.inbox || 0
      } catch (e) {}
    }
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
}
