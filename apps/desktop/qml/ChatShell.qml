import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Chat is the app. Everything else is reachable from the top bar overflow menu.
Item {
    id: shell

    // Inspector visibility is view state: it is not part of the conversation
    // model and never reaches Rust.
    property bool inspectorVisible: false

    signal settingsRequested()
    signal characterLibraryRequested()
    signal characterDetailRequested(string characterId)

    Drawer {
        id: historyDrawer

        width: Math.min(320, shell.width * 0.85)
        height: shell.height
        edge: Qt.LeftEdge

        HistoryPanel {
            anchors.fill: parent
            onSessionSelected: (sessionId) => historyDrawer.close()
            onNewChatRequested: historyDrawer.close()
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        HistoryPanel {
            id: sidebar

            Layout.preferredWidth: 280
            Layout.fillHeight: true
            visible: shell.width >= 900

            onSessionSelected: (sessionId) => {}
            onNewChatRequested: {}
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            ChatTopBar {
                Layout.fillWidth: true

                onHistoryRequested: historyDrawer.open()
                onCharacterRequested: characterSheet.open()
                onModelRequested: modelSheet.open()
                onContextRequested: shell.toggleInspector()
                onSettingsRequested: shell.settingsRequested()
                onCharacterLibraryRequested: shell.characterLibraryRequested()
            }

            ConversationView {
                Layout.fillWidth: true
                Layout.fillHeight: true
            }

            Composer {
                id: composer

                Layout.fillWidth: true
            }
        }

        Pane {
            Layout.preferredWidth: 320
            Layout.fillHeight: true
            visible: shell.width >= 900 && shell.inspectorVisible

            ContextPanel {}
        }
    }

    function toggleInspector() {
        if (shell.width >= 900) {
            shell.inspectorVisible = !shell.inspectorVisible;
            return;
        }
        narrowInspector.open();
    }

    ModelSelectorSheet {
        id: modelSheet
    }

    CharacterSelectorSheet {
        id: characterSheet

        onLibraryRequested: shell.characterLibraryRequested()
    }

    Dialog {
        id: narrowInspector

        title: qsTr("Context & sources")
        width: Math.min(520, shell.width - 48)
        height: Math.min(640, shell.height - 96)
        anchors.centerIn: Overlay.overlay
        modal: true

        ContextPanel {}
    }
}
