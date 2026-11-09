import QtQuick
import Quickshell
import Quickshell.Wayland
import Nyx.Shell
import Nyx.Ui

// Nyx Notification Center in the shared Quickshell runtime. Its backend
// (nyx-notification-center-backend) owns the bus name and Panel1, talks to nyx-notifyd
// and decides when the panel is open; this draws it: a 400 px card in the top-right
// corner of the focused screen, as tall as what it shows, with a clear catcher behind
// it that closes it on a click anywhere else, and the pop-ups. The layer namespaces are
// the GTK panel's.
Scope {
    id: root

    readonly property string pinnedScreen: Quickshell.env("NYX_SHELL_SCREEN") || ""
    readonly property bool keyboardOff: Quickshell.env("NYX_SHELL_NO_KEYBOARD") === "1"
    readonly property var panelState: nc.state.panel || ({ open: false, view: "inbox", settings: false, serial: 0, screen: "" })
    readonly property bool open: panelState.open === true && nc.connected
    property string screenName: ""

    BackendClient {
        id: nc
        app: "nyx-notification-center"
        topics: ["panel", "dnd", "mode", "popups"]
    }
    // The Inbox and the pins are read only while the panel is open.
    BackendTopic {
        id: lists
        client: nc
        name: "lists"
        active: root.open
    }

    function hide() {
        nc.request("Hide", {}, null);
    }

    // A new opening: the screen, the view asked for, nothing expanded.
    property int serial: panelState.serial || 0
    property int shownSerial: -1
    function opening() {
        if (!open || shownSerial === serial)
            return;
        shownSerial = serial;
        screenName = pinnedScreen || panelState.screen || (Quickshell.screens[0] ? Quickshell.screens[0].name : "");
        body.reset();
        body.view = panelState.view || "inbox";
        // Screenshot hooks: NYX_NC_TEST_VIEW=inbox|pinned|history|settings, as the GTK
        // panel had, and NYX_NC_TEST_SEARCH=words (history, searched).
        const test = Quickshell.env("NYX_NC_TEST_VIEW");
        if (test === "settings")
            nc.request("Settings", { open: true }, null);
        else if (test)
            body.view = test;
        const words = Quickshell.env("NYX_NC_TEST_SEARCH");
        if (words) {
            body.view = "history";
            body.setSearch(words);
        }
    }
    onSerialChanged: Qt.callLater(opening)
    onOpenChanged: if (open)
        Qt.callLater(opening)

    readonly property var targetScreen: Quickshell.screens.find(s => s.name === screenName) || null

    PanelWindow {
        id: catcher
        screen: root.targetScreen
        visible: root.open && root.targetScreen !== null
        color: Qt.rgba(0, 0, 0, 0.004)
        anchors.top: true
        anchors.bottom: true
        anchors.left: true
        anchors.right: true
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "nyx-notification-center-catcher"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
            onPressed: root.hide()
        }
    }

    PanelWindow {
        id: win
        screen: root.targetScreen
        // After the catcher, so it stacks above it.
        visible: catcher.visible && catcherShown
        property bool catcherShown: false
        Connections {
            target: catcher
            function onVisibleChanged() {
                if (catcher.visible)
                    Qt.callLater(() => win.catcherShown = true);
                else
                    win.catcherShown = false;
            }
        }
        color: "transparent"
        anchors.top: true
        anchors.right: true
        margins.top: 6
        margins.right: 6
        exclusionMode: ExclusionMode.Normal
        exclusiveZone: 0
        implicitWidth: 400
        implicitHeight: body.implicitHeight + 30
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "nyx-notification-center"
        WlrLayershell.keyboardFocus: root.keyboardOff ? WlrKeyboardFocus.None : WlrKeyboardFocus.OnDemand

        Surface {
            anchors.fill: parent
            shadow: false
            outline: true
            Panel {
                id: body
                x: 14
                y: 14
                width: parent.width - 28
                focus: true
                client: nc
                lists: lists.value || ({ inbox: [], pinned: [], error: null })
                dnd: nc.state.dnd === true
                mode: nc.state.mode || "omarchy"
                settingsOpen: root.panelState.settings === true
                shown: root.open
                maxListHeight: Math.max(200, (root.targetScreen ? root.targetScreen.height : 900) - 220)
                onHide: root.hide()
                onSetSettingsOpen: on => nc.request("Settings", { open: on }, null)
            }
        }
    }

    Popups {
        client: nc
        pinnedScreen: root.pinnedScreen
    }
}
