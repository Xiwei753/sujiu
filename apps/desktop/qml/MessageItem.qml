import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// One conversation turn. Message actions live behind the overflow button so no
// permanent action row sits under every message.
Item {
    id: root

    required property var message

    property bool detailsExpanded: false

    readonly property bool isUser: message.role === "user"
    readonly property var toolCalls: message.toolCalls ? message.toolCalls : []

    implicitHeight: layout.implicitHeight
    width: parent ? parent.width : 0

    Rectangle {
        id: userBlock

        anchors.right: parent.right
        width: Math.min(layout.implicitWidth + 28, root.width * 0.82)
        height: layout.implicitHeight + 20
        radius: 12
        visible: root.isUser
        color: root.palette.alternateBase

        TextEdit {
            anchors.fill: parent
            anchors.margins: 10
            text: root.message.text
            readOnly: true
            selectByMouse: true
            wrapMode: TextEdit.WordWrap
            color: root.palette.text

        }
    }

    ColumnLayout {
        id: layout

        anchors.fill: parent
        spacing: 8

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: root.isUser ? 0 : Math.max(label.implicitHeight, 22)
            visible: !root.isUser

            RowLayout {
                anchors.fill: parent
                spacing: 6

                Label {
                    id: label

                    text: root.message.streaming ? qsTr("Assistant · generating") : qsTr("Assistant")
                    font.pixelSize: 12
                    opacity: 0.6
                    Layout.fillWidth: true
                }

                ToolButton {
                    text: qsTr("⋯")
                    font.pixelSize: 14
                    onClicked: messageMenu.popup()

                    Menu {
                        id: messageMenu

                        y: parent.height
                        MenuItem {
                            text: qsTr("Copy")
                            onTriggered: App.copyMessage(root.message.text)
                        }
                    }
                }
            }
        }

        TextEdit {
            id: body

            Layout.fillWidth: true
            Layout.preferredHeight: implicitHeight
            visible: !root.isUser

            text: root.message.text
            readOnly: true
            selectByMouse: true
            wrapMode: TextEdit.WordWrap
            color: root.palette.text

        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: root.toolCalls.length > 0 ? toolsColumn.implicitHeight : 0
            visible: !root.isUser && root.toolCalls.length > 0

            ColumnLayout {
                id: toolsColumn

                width: parent.width
                spacing: 6

                Button {
                    flat: true
                    onClicked: root.detailsExpanded = !root.detailsExpanded

                    contentItem: RowLayout {
                        spacing: 6
                        Label {
                            text: root.detailsExpanded ? "⌄" : "›"
                            opacity: 0.6
                        }
                        Label {
                            text: qsTr("%1 tool call(s)").arg(root.toolCalls.length)
                            font.pixelSize: 12
                            opacity: 0.8
                        }
                    }
                }

                Repeater {
                    model: root.detailsExpanded ? root.toolCalls : []

                    ColumnLayout {
                        required property var modelData

                        Layout.fillWidth: true
                        spacing: 2

                        Label {
                            text: modelData.title + " · " + modelData.name
                            font.pixelSize: 12
                            font.bold: true
                        }
                        Label {
                            text: modelData.argumentsPreview
                            font.family: "monospace"
                            font.pixelSize: 11
                            opacity: 0.7
                            wrapMode: Text.WordWrap
                            Layout.fillWidth: true
                        }
                        Label {
                            text: modelData.resultPreview ? modelData.resultPreview : qsTr("running…")
                            font.pixelSize: 11
                            opacity: 0.7
                            wrapMode: Text.WordWrap
                            Layout.fillWidth: true
                        }
                    }
                }
            }
        }
    }
}
