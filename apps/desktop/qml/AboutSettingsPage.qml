import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// About is information, not a control: two lines and nothing to press. Nothing
// here gets a chevron, because a row that looks tappable and is not is worse
// than a plain line.
Page {
    id: aboutPage

    signal backRequested()

    header: ToolBar {
        RowLayout {
            anchors.fill: parent

            ToolButton {
                text: qsTr("Back")
                onClicked: aboutPage.backRequested()
            }

            Label {
                text: qsTr("About")
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
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.topMargin: 16
        spacing: 6

        Label {
            text: "Sujiu 0.1.0"
        }

        Label {
            text: App.platformSummary
            font.pixelSize: 11
            opacity: 0.6
        }
    }
}
