import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Standard settings pattern. Sections appear as the screens that implement them
// land, instead of a grid of decorative cards.
Page {
    id: page

    signal backRequested()

    header: ToolBar {
        RowLayout {
            anchors.fill: parent

            ToolButton {
                text: "‹"
                font.pixelSize: 20
                onClicked: page.backRequested()
            }

            Label {
                text: qsTr("Settings")
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
        spacing: 4

        model: 2

        delegate: Loader {
            required property int index

            width: parent ? parent.width : 0

            sourceComponent: index === 0 ? appearanceComponent : aboutComponent
        }

        Component {
            id: appearanceComponent

            ColumnLayout {
                width: parent ? parent.width : 0
                spacing: 4

                Label {
                    text: qsTr("Appearance")
                    font.pixelSize: 12
                    opacity: 0.6
                    Layout.leftMargin: 12
                }

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 6
                    spacing: 8

                    Repeater {
                        model: ["system", "light", "dark"]

                        Button {
                            required property var modelData

                            text: modelData
                            checkable: true
                            checked: App.appearanceMode === modelData
                            onClicked: App.appearanceMode = modelData
                        }
                    }
                }
            }
        }

        Component {
            id: aboutComponent

            ColumnLayout {
                width: parent ? parent.width : 0
                spacing: 4

                Label {
                    text: qsTr("About")
                    font.pixelSize: 12
                    opacity: 0.6
                    Layout.leftMargin: 12
                }

                Label {
                    text: "Sujiu 0.1.0"
                    Layout.leftMargin: 12
                }

                Label {
                    text: App.platformSummary
                    font.pixelSize: 11
                    opacity: 0.6
                    Layout.leftMargin: 12
                }
            }
        }
    }
}
