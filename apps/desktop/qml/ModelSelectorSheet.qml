import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Same selection pattern as the character selector: show the current model,
// list candidates, keep provider configuration in settings.
Popup {
    id: popup

    y: 0
    x: 0
    width: Math.min(420, parent ? parent.width - 32 : 420)
    padding: 12
    modal: true

    contentItem: ColumnLayout {
        spacing: 8

        Label {
            text: qsTr("Model")
            font.bold: true
        }

        Repeater {
            model: App.models

            ItemDelegate {
                required property var modelData

                width: parent.width
                highlighted: modelData.id === App.currentModelId
                enabled: modelData.available
                onClicked: {
                    App.selectModel(modelData.id);
                    popup.close();
                }

                contentItem: ColumnLayout {
                    spacing: 0

                    Label {
                        text: modelData.name
                        opacity: modelData.available ? 1 : 0.5
                    }
                    Label {
                        text: modelData.providerLabel + " · " + modelData.capabilities.join(", ")
                        font.pixelSize: 11
                        opacity: 0.6
                    }
                }
            }
        }
    }
}
