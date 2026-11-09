import QtQuick
import Nyx.Ui

// A settings row: title and explanation (or an extra control under the title) on the
// left, a control on the right.
Item {
    id: o

    property string title: ""
    property string sub: ""
    property Item extra: null
    default property alias control: right.data

    width: parent ? parent.width : 300
    implicitHeight: Math.max(text.implicitHeight, right.childrenRect.height) + 16

    Column {
        id: text
        x: 8
        anchors.verticalCenter: parent.verticalCenter
        width: o.width - 16 - right.childrenRect.width - 10
        spacing: 2
        Label {
            width: parent.width
            text: o.title
            size: 13
            font.weight: Font.Medium
        }
        Label {
            visible: o.sub !== ""
            width: parent.width
            text: o.sub
            size: 12
            color: Style.dimText
            wrapMode: Text.Wrap
        }
        Item {
            id: extraHolder
            visible: o.extra !== null
            width: parent.width
            height: o.extra ? o.extra.implicitHeight + 4 : 0
        }
    }
    function placeExtra() {
        if (extra) {
            extra.parent = extraHolder;
            extra.y = 4;
        }
    }
    onExtraChanged: placeExtra()
    Component.onCompleted: placeExtra()
    Item {
        id: right
        anchors.right: parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        width: childrenRect.width
        height: childrenRect.height
    }
}
