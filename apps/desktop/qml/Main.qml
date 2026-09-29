import QtQuick
import QtQuick.Controls

ApplicationWindow {
    id: window

    width: 1100
    height: 760
    visible: true
    title: "Sujiu"

    Column {
        anchors.centerIn: parent
        spacing: 12

        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "Sujiu"
            font.pixelSize: 32
        }

        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "Desktop shell · Rust core wiring comes next"
        }
    }
}
