import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Chat is the home surface. The top bar's five slots are the whole of the
// top-level information architecture; nothing else needs an entry from here.
Item {
    id: shell

    // Inspector visibility is view state: it is not part of the conversation
    // model and never reaches Rust. It currently has no toggle — see the pane
    // at the bottom of this file.
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
                onModelRequested: modelSheet.open()
                onSettingsRequested: shell.settingsRequested()
                onLibraryRequested: shell.characterLibraryRequested()
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

        // The context inspector has no entry point yet. Its one spec'd entry is
        // the conversation title → this conversation's contents, and this
        // frontend has no contents page; it used to hang off the three-dot
        // overflow, which this issue removed. The pane and the flag stay
        // because the destination is correct and the page that reaches it is
        // the only missing piece — apps/desktop/TODO.md records it.
        Pane {
            Layout.preferredWidth: 320
            Layout.fillHeight: true
            visible: shell.width >= 900 && shell.inspectorVisible

            ContextPanel {}
        }
    }

    ModelSelectorSheet {
        id: modelSheet
    }
}
