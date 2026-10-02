import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Appearance: three mutually exclusive options, so a list of checkable
// delegates rather than a row of toggle buttons.
//
// The old row of `Button { checkable: true }` showed every option in the same
// visual state apart from a border, so it read as three switches rather than as
// one selection. `ItemDelegate.checkable` is the control that already means
// "exactly one of these", and it lets the style draw the indicator.
Page {
    id: appearancePage

    signal backRequested()

    readonly property var modes: [
        { key: "system", label: qsTr("System") },
        { key: "light", label: qsTr("Light") },
        { key: "dark", label: qsTr("Dark") }
    ]

    header: ToolBar {
        RowLayout {
            anchors.fill: parent

            ToolButton {
                text: qsTr("Back")
                onClicked: appearancePage.backRequested()
            }

            Label {
                text: qsTr("Appearance")
                font.pixelSize: 16
                font.bold: true
            }

            Item {
                Layout.fillWidth: true
            }
        }
    }

    ListView {
        anchors.fill: parent
        contentWidth: width
        clip: true
        spacing: 0

        model: appearancePage.modes

        delegate: ItemDelegate {
            required property var modelData

            width: ListView.view.width
            text: modelData.label
            checkable: true
            checked: App.appearanceMode === modelData.key
            onClicked: App.appearanceMode = modelData.key
        }
    }
}
