import QtQuick
import Nyx.Ui

// A small heading over a part of a list.
Label {
    property string title: ""
    text: title.toUpperCase()
    size: 11
    font.weight: Font.DemiBold
    font.letterSpacing: 0.66
    color: Style.tertiary
    leftPadding: 4
    topPadding: 6
}
