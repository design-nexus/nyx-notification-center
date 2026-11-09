import QtQuick
import Quickshell
import Nyx.Ui

// An app's icon: its theme icon or file, else Omarchy's glyph, else a generic one.
Item {
    id: a

    property string icon: ""
    property string glyph: ""
    property int size: 30
    readonly property string path: {
        if (icon.startsWith("/") || icon.startsWith("file://"))
            return icon.startsWith("/") ? "file://" + icon : icon;
        return icon !== "" ? Quickshell.iconPath(icon, true) : "";
    }

    implicitWidth: size
    implicitHeight: size

    Image {
        id: img
        anchors.fill: parent
        visible: a.path !== "" && status === Image.Ready
        source: a.path
        sourceSize: Qt.size(a.size * 2, a.size * 2)
        fillMode: Image.PreserveAspectFit
        asynchronous: true
        smooth: true
        mipmap: true
    }
    Rectangle {
        anchors.fill: parent
        visible: !img.visible
        radius: 8
        color: Style.alpha(Style.text, 0.1)
        Label {
            anchors.centerIn: parent
            visible: a.glyph !== ""
            text: a.glyph
            font.family: "JetBrainsMono Nerd Font"
            size: a.size * 0.5
        }
        Icon {
            anchors.centerIn: parent
            visible: a.glyph === ""
            name: "application-x-executable-symbolic"
            size: Math.floor(a.size / 2) + 1
            color: Style.text
        }
    }
}
