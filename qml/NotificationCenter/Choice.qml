import QtQuick
import Nyx.Ui

// A pill showing the chosen item; a click opens the list (history's filters, the quiet
// hours' times).
Rectangle {
    id: ch

    property var items: []
    property int index: 0
    property bool pill: true
    property int menuWidth: 180

    signal chosen(int index)

    implicitWidth: row.implicitWidth + 18
    implicitHeight: 26
    radius: pill ? 13 : 7
    color: pill ? Style.card : Style.alpha(Style.text, 0.06)

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 4
        Label {
            anchors.verticalCenter: parent.verticalCenter
            text: ch.items[ch.index] || ""
            size: 12
            color: ch.pill && !area.containsMouse ? Style.dimText : Style.text
            font.features: { "tnum": 1 }
        }
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            name: "pan-down-symbolic"
            size: 12
            color: Style.dimText
        }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: menu.openAt(0, ch.height + 4)
    }
    Menu {
        id: menu
        anchorItem: ch
        minWidth: ch.menuWidth
        current: ch.index
        entries: ch.items.map((t, i) => ({ id: String(i), label: t, checked: i === ch.index }))
        onTriggered: id => ch.chosen(parseInt(id))
    }
}
