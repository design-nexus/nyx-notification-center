import QtQuick
import Nyx.Ui

// A list of rows: cards, headings, folded groups and their heads, an empty line. Built
// by the views from what nyx-notifyd sent; scrolls when taller than `maxHeight`.
Flickable {
    id: l

    required property var client
    property var rows: []
    property real maxHeight: 600
    property var words: []
    property real now: Date.now() / 1000
    property string place: "inbox"

    signal done
    signal expand(string app)
    signal fold(string app)
    signal showPinned

    implicitHeight: Math.min(col.implicitHeight, maxHeight)
    contentHeight: col.implicitHeight
    clip: true
    boundsBehavior: Flickable.StopAtBounds

    Column {
        id: col
        width: l.width
        spacing: 8
        bottomPadding: 4
        Repeater {
            model: l.rows
            delegate: Loader {
                id: row
                required property var modelData
                width: col.width
                sourceComponent: ({ card: cardC, section: sectionC, pinnedHead: pinnedHeadC, groupHead: groupHeadC, stack: stackC, empty: emptyC })[modelData.kind] || null
            }
        }
    }

    Component {
        id: cardC
        NoteCard {
            readonly property var modelData: parent ? parent.modelData : ({})
            note: modelData.note
            client: l.client
            place: modelData.place || l.place
            words: l.words
            now: l.now
            onDone: l.done()
        }
    }
    Component {
        id: sectionC
        Section {
            readonly property var modelData: parent ? parent.modelData : ({})
            title: modelData.text
        }
    }
    Component {
        id: emptyC
        Empty {
            readonly property var modelData: parent ? parent.modelData : ({})
            icon: modelData.icon
            text: modelData.text
        }
    }
    Component {
        id: pinnedHeadC
        Item {
            implicitHeight: 24
            Section {
                anchors.verticalCenter: parent.verticalCenter
                title: "Pinned"
                topPadding: 0
            }
            IconButton {
                anchors.right: parent.right
                width: 24
                height: 24
                icon: "go-next-symbolic"
                tooltip: "All pinned"
                onClicked: l.showPinned()
            }
        }
    }
    Component {
        id: groupHeadC
        Item {
            readonly property var modelData: parent ? parent.modelData : ({})
            implicitHeight: 28
            Row {
                x: 4
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                AppIcon {
                    anchors.verticalCenter: parent.verticalCenter
                    icon: modelData.note.app_icon || ""
                    glyph: modelData.note.glyph || ""
                    size: 18
                }
                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.note.name || ""
                    size: 12.5
                    font.weight: Font.DemiBold
                }
            }
            Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                IconButton {
                    width: 24
                    height: 24
                    icon: "edit-clear-all-symbolic"
                    tooltip: "Clear these"
                    onClicked: l.client.request("Dismiss", { ids: modelData.ids }, null)
                }
                IconButton {
                    width: 24
                    height: 24
                    icon: "pan-up-symbolic"
                    tooltip: "Show less"
                    onClicked: l.fold(modelData.app)
                }
            }
        }
    }
    // A group folded up: its newest card with the edges of the others under it.
    Component {
        id: stackC
        Item {
            id: st
            readonly property var modelData: parent ? parent.modelData : ({})
            implicitHeight: top.height + 12
            NoteCard {
                id: top
                width: parent.width
                note: st.modelData.note
                client: l.client
                place: "inbox"
                now: l.now
                Rectangle {
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.rightMargin: 12
                    anchors.bottomMargin: 10
                    width: count.implicitWidth + 14
                    height: count.implicitHeight + 2
                    radius: 9
                    color: Style.alpha(Style.text, 0.1)
                    Label {
                        id: count
                        anchors.centerIn: parent
                        text: String(st.modelData.count)
                        size: 11
                        color: Style.dimText
                    }
                }
            }
            Rectangle {
                y: top.height - 11
                x: 10
                z: -1
                width: parent.width - 20
                height: 17
                radius: 11
                color: Style.mix(Style.card, Style.text, 0.09)
            }
            Rectangle {
                y: top.height - 5
                x: 22
                z: -2
                width: parent.width - 44
                height: 17
                radius: 11
                color: Style.mix(Style.card, Style.text, 0.06)
            }
            // Any click opens the group (before the card would act on it).
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: l.expand(st.modelData.app)
            }
            Tip {
                target: st
                text: st.modelData.count + " notifications from " + (st.modelData.note.name || "")
                shown: top.hovered
            }
        }
    }
}
