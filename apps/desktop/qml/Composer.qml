import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// `+` opens the attachment/capability sheet; nothing else is permanently pinned
// above the input.
Pane {
    id: composer

    padding: 12

    Menu {
        id: attachmentMenu

        y: -implicitHeight - 4
        x: 0

        MenuItem {
            text: qsTr("Image or file")
            enabled: false
        }
        MenuItem {
            text: qsTr("Tools for this chat")
            enabled: false
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 8

        ToolButton {
            text: "+"
            font.pixelSize: 20
            onClicked: attachmentMenu.popup()
        }

        TextField {
            id: input

            Layout.fillWidth: true
            placeholderText: qsTr("Send a message")
            text: App.draft
            onTextEdited: App.draft = text
            onAccepted: {
                if (!App.busy) {
                    App.send();
                }
            }

            Keys.onEscapePressed: input.text = ""
        }

        Button {
            text: App.busy ? qsTr("Stop") : qsTr("Send")
            enabled: App.busy || App.draft.trim().length > 0
            onClicked: {
                if (App.busy) {
                    App.cancel();
                } else {
                    App.send();
                    input.text = "";
                }
            }
        }
    }
}
