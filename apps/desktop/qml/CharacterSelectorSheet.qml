import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Recents first, search available, full library one click away.
Popup {
    id: popup

    y: 0
    x: 0
    width: Math.min(420, parent ? parent.width - 32 : 420)
    height: Math.min(560, parent ? parent.height - 64 : 560)
    padding: 12
    modal: true

    signal libraryRequested()

    contentItem: ColumnLayout {
        spacing: 8

        Label {
            text: qsTr("Character")
            font.bold: true
        }

        TextField {
            Layout.fillWidth: true
            placeholderText: qsTr("Search characters")
            onTextChanged: App.searchCharacters(text)
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true

            ColumnLayout {
                width: parent.width
                spacing: 2

                Repeater {
                    model: App.characters

                    ItemDelegate {
                        required property var modelData

                        width: parent.width
                        highlighted: modelData.id === App.currentCharacterId
                        onClicked: {
                            App.selectCharacter(modelData.id);
                            popup.close();
                        }

                        contentItem: ColumnLayout {
                            spacing: 0

                            Label {
                                text: modelData.name
                            }
                            Label {
                                text: modelData.tagline
                                font.pixelSize: 11
                                opacity: 0.6
                            }
                        }
                    }
                }
            }
        }

        Button {
            Layout.fillWidth: true
            text: qsTr("Open character library")
            onClicked: {
                popup.close();
                popup.libraryRequested();
            }
        }
    }

    onClosed: App.searchCharacters("")
}
