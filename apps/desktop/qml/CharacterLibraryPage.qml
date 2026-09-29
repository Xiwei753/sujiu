import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Secondary page. Cards instead of a contacts list: this is a content library.
Page {
    id: page

    signal backRequested()
    signal characterSelected(string characterId)

    header: ToolBar {
        RowLayout {
            anchors.fill: parent

            ToolButton {
                text: "‹"
                font.pixelSize: 20
                onClicked: page.backRequested()
            }

            Label {
                text: qsTr("Characters")
                font.pixelSize: 16
                font.bold: true
            }

            Item {
                Layout.fillWidth: true
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        TextField {
            id: search

            Layout.fillWidth: true
            Layout.margins: 12
            placeholderText: qsTr("Search characters")
            onTextChanged: App.searchCharacters(text)
        }

        GridView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            cellWidth: 220
            cellHeight: 140
            model: App.characters

            delegate: Rectangle {
                required property var modelData

                width: cellWidth - 16
                height: cellHeight - 16
                radius: 10
                color: page.palette.alternateBase
                border.color: page.palette.mid
                border.width: 0

                MouseArea {
                    anchors.fill: parent
                    onClicked: page.characterSelected(modelData.id)
                }

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 12
                    spacing: 4

                    Label {
                        text: modelData.name
                        font.bold: true
                    }
                    Label {
                        text: modelData.tagline
                        font.pixelSize: 11
                        opacity: 0.7
                        elide: Text.ElideRight
                    }
                    Item {
                        Layout.fillHeight: true
                    }
                    Label {
                        text: qsTr("Open")
                        font.pixelSize: 11
                        opacity: 0.6
                    }
                }
            }
        }
    }

    Component.onCompleted: search.text = ""
}
