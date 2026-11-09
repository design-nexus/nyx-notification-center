import QtQuick
import Quickshell
import Quickshell.Wayland
import Nyx.Ui

// Pop-ups in the top-right corner of the focused screen, newest on top. The backend
// keeps their time; the pointer on one holds it. Only when Nyx shows pop-ups.
PanelWindow {
    id: w

    required property var client
    property string pinnedScreen: ""
    readonly property var state: client.state.popups || ({ screen: "", items: [] })
    readonly property var items: state.items || []
    readonly property var target: Quickshell.screens.find(s => s.name === (pinnedScreen || state.screen)) || Quickshell.screens[0] || null

    visible: items.length > 0 && target !== null
    screen: target
    anchors.top: true
    anchors.right: true
    margins.top: 8
    margins.right: 8
    exclusionMode: ExclusionMode.Normal
    exclusiveZone: 0
    color: "transparent"
    implicitWidth: 380 + 8
    implicitHeight: Math.max(1, list.implicitHeight)
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nyx-notification-popups"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    Column {
        id: list
        x: 4
        y: 2
        width: 380
        spacing: 12
        bottomPadding: 10
        Repeater {
            model: ScriptModel {
                values: w.items.map(i => i.id)
            }
            delegate: NoteCard {
                id: card
                required property var modelData
                readonly property var item: w.items.find(i => i.id === modelData) || ({ note: ({ id: modelData }), hide_text: false })
                width: 380
                note: item.note
                hideText: item.hide_text
                client: w.client
                place: "popup"
                onHoveredChanged: w.client.request("PopupHold", { id: modelData, on: hovered }, null)
            }
        }
    }
}
