import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Plain hierarchical detail. No progression dashboard, no stat blocks.
Page {
    id: page

    signal backRequested()

    property string characterId: ""

    readonly property var character: {
        const characters = App.characters;
        for (let index = 0; index < characters.length; index += 1) {
            if (characters[index].id === page.characterId) {
                return characters[index];
            }
        }
        return null;
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent

            ToolButton {
                text: "‹"
                font.pixelSize: 20
                onClicked: page.backRequested()
            }

            Label {
                text: page.character ? page.character.name : qsTr("Character")
                font.pixelSize: 16
                font.bold: true
            }

            Item {
                Layout.fillWidth: true
            }
        }
    }

    ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth
        clip: true

        Column {
            width: parent.width
            spacing: 16
            topPadding: 20
            bottomPadding: 20
            leftPadding: 20
            rightPadding: 20

            Label {
                text: page.character ? page.character.description : ""
                opacity: 0.7
                visible: text.length > 0
            }

            Label {
                text: qsTr("Greeting")
                font.pixelSize: 12
                opacity: 0.6
            }

            Label {
                width: parent.width
                text: page.character ? page.character.greeting : ""
                wrapMode: Text.WordWrap
            }

            Label {
                text: qsTr("Editing, world book and preset overrides arrive with the character editor.")
                font.pixelSize: 11
                opacity: 0.5
                wrapMode: Text.WordWrap
                width: parent.width
            }
        }
    }
}
