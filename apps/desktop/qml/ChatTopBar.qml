import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Chat top bar: exactly five slots, in the order `docs/UI_ARCHITECTURE.md` §1.2
// fixes.
//
//     历史入口 | 会话标题 | 模型            资料库 | 设置
//
// There is deliberately no overflow menu. It used to be the only place library
// and settings were reachable from, which meant the two most-used destinations
// in the app were hidden behind three dots next to a wide, empty toolbar.
//
// Every slot is a flat `ToolButton` with a text label rather than a glyph.
// Qt Quick Controls ships no icon set, and this project carries no icon
// resources, so an "icon" here could only be a Unicode character standing in
// for one — the thing that reads differently in every font and tells a
// screen reader nothing. A label says what it is; if a real icon theme is
// added later it replaces the label, not the other way round.
ToolBar {
    id: topBar

    signal historyRequested()
    signal modelRequested()
    signal libraryRequested()
    signal settingsRequested()

    RowLayout {
        anchors.fill: parent
        spacing: 4

        ToolButton {
            text: qsTr("Chats")
            onClicked: topBar.historyRequested()
            ToolTip.visible: hovered
            ToolTip.text: qsTr("Chat history")
        }

        // The conversation title is the entry to this conversation's contents
        // and bindings. This frontend has no such page yet, so the slot is a
        // label rather than a control that opens the character picker — see
        // apps/desktop/TODO.md.
        Label {
            text: App.currentCharacterName
            elide: Text.ElideRight
            Layout.maximumWidth: 220
            Layout.leftMargin: 8
        }

        ToolButton {
            text: App.currentModelName
            onClicked: topBar.modelRequested()
            Layout.maximumWidth: 200
        }

        Item {
            Layout.fillWidth: true
        }

        ToolButton {
            text: qsTr("Library")
            onClicked: topBar.libraryRequested()
            ToolTip.visible: hovered
            ToolTip.text: qsTr("Characters, Personas, world books and prompts")
        }

        ToolButton {
            text: qsTr("Settings")
            onClicked: topBar.settingsRequested()
            ToolTip.visible: hovered
            ToolTip.text: qsTr("App and service configuration")
        }
    }
}
