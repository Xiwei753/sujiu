import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// History is a list, so it is presented as a list — never as the home surface.
Pane {
    id: panel

    signal sessionSelected(string sessionId)
    signal newChatRequested()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 8

        Button {
            Layout.fillWidth: true
            text: qsTr("New chat")
            onClicked: panel.newChatRequested()
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true

            ColumnLayout {
                width: parent.width
                spacing: 4

                Repeater {
                    model: App.sessionGroups

                    ColumnLayout {
                        required property var modelData

                        Layout.fillWidth: true
                        spacing: 2

                        Label {
                            text: modelData.label
                            font.pixelSize: 12
                            opacity: 0.6
                            Layout.leftMargin: 6
                        }

                        Repeater {
                            model: modelData.sessions

                            ItemDelegate {
                                required property var modelData

                                width: parent.width
                                highlighted: modelData.id === App.currentSessionId
                                text: modelData.title
                                onClicked: panel.sessionSelected(modelData.id)
                            }
                        }
                    }
                }
            }
        }
    }
}
