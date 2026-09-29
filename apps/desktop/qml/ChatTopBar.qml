import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Lightweight top bar: history, current character, current model, overflow.
ToolBar {
    id: topBar

    signal historyRequested()
    signal characterRequested()
    signal modelRequested()
    signal contextRequested()
    signal settingsRequested()
    signal characterLibraryRequested()

    RowLayout {
        anchors.fill: parent
        spacing: 4

        ToolButton {
            text: qsTr("☰")
            font.pixelSize: 18
            onClicked: topBar.historyRequested()
            ToolTip.visible: hovered
            ToolTip.text: qsTr("Chat history")
        }

        Button {
            Layout.maximumWidth: 220
            flat: true
            onClicked: topBar.characterRequested()

            contentItem: RowLayout {
                spacing: 6
                Label {
                    text: App.currentCharacterName
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                Label {
                    text: "⌄"
                    opacity: 0.6
                }
            }
        }

        Button {
            flat: true
            onClicked: topBar.modelRequested()

            contentItem: RowLayout {
                spacing: 6
                Label {
                    text: App.currentModelName
                    elide: Text.ElideRight
                    Layout.maximumWidth: 180
                }
                Label {
                    text: "⌄"
                    opacity: 0.6
                }
            }
        }

        Item {
            Layout.fillWidth: true
        }

        ToolButton {
            id: overflowButton

            text: qsTr("⋯")
            font.pixelSize: 18
            onClicked: overflowMenu.popup()

            Menu {
                id: overflowMenu

                y: overflowButton.height
                x: -width + overflowButton.width

                MenuItem {
                    text: qsTr("Context & sources")
                    onTriggered: topBar.contextRequested()
                }
                MenuItem {
                    text: qsTr("Character library")
                    onTriggered: topBar.characterLibraryRequested()
                }
                MenuSeparator {}
                MenuItem {
                    text: qsTr("Settings")
                    onTriggered: topBar.settingsRequested()
                }
            }
        }
    }
}
