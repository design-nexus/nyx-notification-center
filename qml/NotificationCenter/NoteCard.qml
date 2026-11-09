import QtQuick
import Nyx.Ui
import "util.js" as U

// One notification: the app's icon, who and when, the title and text, its buttons.
// Hovering shows pin and dismiss (or delete); clicking runs what the sender asked for,
// or opens the app (nyx-notifyd decides), and closes the panel if anything happened.
Rectangle {
    id: c

    required property var note
    required property var client
    // "inbox", "pinned", "history" or "popup".
    property string place: "inbox"
    property var words: []
    property real now: Date.now() / 1000
    property bool hideText: false
    readonly property bool popup: place === "popup"
    readonly property bool unread: !note.read && place === "inbox"
    readonly property bool hovered: hover.hovered
    readonly property bool rich: words.length > 0
    readonly property string highlight: "rgba(122,162,247,0.33)"

    signal done

    function act(op, body) {
        client.request(op, body, null);
    }

    implicitHeight: row.implicitHeight + (popup ? 24 : 21)
    radius: popup ? 15 : 11
    color: popup ? (hovered ? Style.mix(Style.popover, Style.text, 0.04) : Style.popover) : (hovered ? Style.mix(Style.card, Style.text, 0.04) : Style.card)
    border.width: popup ? 1 : 0
    border.color: Style.border
    Behavior on color {
        ColorAnimation {
            duration: Style.reducedMotion ? 0 : 120
        }
    }

    HoverHandler {
        id: hover
        cursorShape: c.note.clickable ? Qt.PointingHandCursor : Qt.ArrowCursor
    }
    TapHandler {
        acceptedButtons: Qt.LeftButton
        onTapped: c.client.request("Activate", { id: c.note.id }, ok => {
            if (ok === true)
                c.done();
        })
    }

    Row {
        id: row
        x: c.popup ? 14 : 12
        y: c.popup ? 12 : 10
        width: c.width - 2 * x
        spacing: 10

        Item {
            width: 30
            height: 30
            Rectangle {
                id: pictureFrame
                anchors.fill: parent
                visible: c.note.image !== ""
                radius: 8
                clip: true
                color: "transparent"
                Image {
                    anchors.fill: parent
                    source: c.note.image !== "" ? "file://" + c.note.image : ""
                    sourceSize: Qt.size(60, 60)
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                }
            }
            AppIcon {
                visible: !pictureFrame.visible
                anchors.fill: parent
                icon: c.note.app_icon || ""
                glyph: c.note.glyph || ""
                size: 30
            }
        }

        Column {
            width: row.width - 40
            spacing: 1

            Item {
                width: parent.width
                height: appLabel.implicitHeight
                Row {
                    spacing: 5
                    Label {
                        id: appLabel
                        text: c.note.name || ""
                        size: 11.5
                        color: Style.dimText
                        font.weight: Font.Medium
                        elide: Text.ElideRight
                        width: Math.min(implicitWidth, c.width * 0.6)
                    }
                    Icon {
                        visible: c.note.pinned && c.place !== "pinned"
                        anchors.verticalCenter: parent.verticalCenter
                        name: "view-pin-symbolic"
                        size: 12
                        color: Style.accent
                    }
                }
                Label {
                    anchors.right: parent.right
                    text: c.popup ? "now" : U.when(c.note.ts, c.now)
                    size: 11.5
                    color: Style.dimText
                    font.features: { "tnum": 1 }
                    opacity: c.hovered ? 0 : 1
                }
            }

            Row {
                width: parent.width
                spacing: 6
                topPadding: 1
                Rectangle {
                    visible: c.unread
                    anchors.verticalCenter: parent.verticalCenter
                    width: 7
                    height: 7
                    radius: 4
                    color: Style.accent
                }
                Label {
                    width: parent.width - (c.unread ? 13 : 0)
                    textFormat: c.rich ? Text.RichText : Text.PlainText
                    text: {
                        const t = c.note.summary || c.note.app || "";
                        return c.rich ? U.marked(t, c.words, c.highlight) : t;
                    }
                    size: 13
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }
            }

            Label {
                width: parent.width
                visible: !!c.note.body && !c.hideText
                textFormat: c.rich ? Text.RichText : Text.PlainText
                text: c.rich ? U.marked(U.flat(c.note.body), c.words, c.highlight) : U.flat(c.note.body)
                size: 12.5
                color: Style.dimText
                wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                maximumLineCount: 3
                elide: Text.ElideRight
            }

            // Buttons: a code to copy, the app's own (while it's open), Omarchy's commands.
            Flow {
                id: acts
                width: parent.width
                spacing: 6
                topPadding: 8
                visible: codes.count + appActs.count + cmdActs.count > 0
                Repeater {
                    id: codes
                    model: c.hideText ? [] : (c.note.codes || []).slice(0, 2)
                    delegate: ActionButton {
                        required property var modelData
                        icon: "edit-copy-symbolic"
                        text: modelData
                        mono: true
                        tooltip: copied ? "Copied" : "Copy the code"
                        property bool copied: false
                        onClicked: {
                            c.act("CopyCode", { code: modelData });
                            copied = true;
                        }
                    }
                }
                Repeater {
                    id: appActs
                    model: c.note.open ? (c.note.app_actions || []).filter(a => a.key !== "default" && String(a.label).trim() !== "").slice(0, 3) : []
                    delegate: ActionButton {
                        required property var modelData
                        text: modelData.label
                        onClicked: {
                            c.act("InvokeAction", { id: c.note.id, key: modelData.key });
                            c.done();
                        }
                    }
                }
                Repeater {
                    id: cmdActs
                    model: (c.note.actions || []).slice(0, 3)
                    delegate: ActionButton {
                        required property var modelData
                        required property int index
                        text: modelData.label
                        onClicked: {
                            c.act("RunAction", { id: c.note.id, index: index });
                            c.done();
                        }
                    }
                }
            }
        }
    }

    // Hover: pin, and dismiss (Inbox, pop-ups) or delete (Pinned, History).
    Rectangle {
        visible: c.hovered
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: 6
        width: hoverRow.implicitWidth
        height: hoverRow.implicitHeight
        radius: 7
        color: c.popup ? Style.popover : Style.card
        Row {
            id: hoverRow
            spacing: 2
            IconButton {
                width: 24
                height: 24
                icon: "view-pin-symbolic"
                toggled: c.note.pinned
                tooltip: c.note.pinned ? "Unpin" : "Pin"
                onClicked: c.act("SetPinned", { id: c.note.id, pinned: !c.note.pinned })
            }
            IconButton {
                width: 24
                height: 24
                icon: c.place === "popup" || c.place === "inbox" ? "window-close-symbolic" : "user-trash-symbolic"
                tooltip: c.place === "popup" || c.place === "inbox" ? "Dismiss" : (c.note.pinned ? "Delete (also unpins)" : "Delete")
                onClicked: {
                    if (c.place === "popup")
                        c.act("PopupDismiss", { id: c.note.id });
                    else if (c.place === "inbox")
                        c.act("Dismiss", { ids: [c.note.id] });
                    else
                        c.act("Delete", { ids: [c.note.id] });
                }
            }
        }
    }
}
