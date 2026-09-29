import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The conversation owns the screen. Assistant text is body content, user turns
// get a light block, and tool activity stays collapsed until asked for.
ScrollView {
    id: view

    clip: true
    contentWidth: availableWidth

    Column {
        width: view.availableWidth
        spacing: 18

        topPadding: 24
        bottomPadding: 24
        leftPadding: 20
        rightPadding: 20

        Repeater {
            model: App.messages

            MessageItem {
                required property var modelData

                message: modelData
            }
        }

        Item {
            width: parent.width
            height: App.busy ? 24 : 0
            visible: App.busy

            RowLayout {
                anchors.fill: parent
                spacing: 8

                BusyIndicator {
                    running: App.busy
                    implicitWidth: 18
                    implicitHeight: 18
                }

                Label {
                    text: App.statusLabel !== "" ? App.statusLabel : qsTr("Generating")
                    opacity: 0.75
                    font.pixelSize: 12
                }
            }
        }
    }
}
