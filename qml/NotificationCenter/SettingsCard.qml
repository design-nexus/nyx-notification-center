import QtQuick
import Nyx.Ui

// A group of settings rows on a card.
Rectangle {
    default property alias rows: col.data

    implicitHeight: col.implicitHeight + 12
    radius: 11
    color: Style.card
    Column {
        id: col
        x: 6
        y: 6
        width: parent.width - 12
    }
}
