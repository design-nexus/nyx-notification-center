import QtQuick
import Nyx.Ui

// Nothing to show (or a problem): an icon and a line, centred.
Column {
    property string icon: "preferences-system-notifications-symbolic"
    property string text: ""

    topPadding: 60
    bottomPadding: 60
    spacing: 8
    Icon {
        anchors.horizontalCenter: parent.horizontalCenter
        name: parent.icon
        size: 30
        color: Style.tertiary
    }
    Label {
        width: parent.width - 40
        anchors.horizontalCenter: parent.horizontalCenter
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        text: parent.text
        size: 12.5
        color: Style.tertiary
    }
}
