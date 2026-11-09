import QtQuick
import Nyx.Ui

// A notification's button: a code to copy, or one of the sender's actions.
Rectangle {
    id: b

    property string text: ""
    property string icon: ""
    property bool mono: false
    property string tooltip: ""

    signal clicked

    implicitWidth: content.implicitWidth + 20
    implicitHeight: 24
    radius: 7
    color: Style.alpha(Style.text, area.containsMouse ? 0.13 : 0.08)

    Row {
        id: content
        anchors.centerIn: parent
        spacing: 6
        Icon {
            visible: b.icon !== ""
            anchors.verticalCenter: parent.verticalCenter
            name: b.icon
            size: 13
            color: Style.text
        }
        Label {
            anchors.verticalCenter: parent.verticalCenter
            text: b.text
            size: 12
            font.weight: Font.Medium
            font.features: b.mono ? { "tnum": 1 } : {}
            font.letterSpacing: b.mono ? 0.5 : 0
        }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: b.clicked()
    }
    Tip {
        target: area
        text: b.tooltip
        shown: b.tooltip !== "" && area.containsMouse
    }
}
