import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Transparency panel for one turn. Sources map to Rust `ContextSource` records
// and tool calls map to the tool records of the latest assistant turn.
ScrollView {
    id: panel

    clip: true

    readonly property var latestAssistant: {
        const messages = App.messages;
        for (let index = messages.length - 1; index >= 0; index -= 1) {
            if (messages[index].role === "assistant") {
                return messages[index];
            }
        }
        return null;
    }

    contentWidth: availableWidth

    ColumnLayout {
        width: panel.availableWidth
        spacing: 12

        Label {
            text: qsTr("Context & sources")
            font.bold: true
        }

        Label {
            text: qsTr("Sources")
            font.pixelSize: 12
            opacity: 0.6
        }

        Repeater {
            model: App.contextSources

            ItemDelegate {
                required property var modelData

                width: parent.width
                highlighted: false
                enabled: false

                contentItem: ColumnLayout {
                    spacing: 0

                    RowLayout {
                        Layout.fillWidth: true

                        Label {
                            text: modelData.label
                            Layout.fillWidth: true
                        }
                        Label {
                            text: modelData.recordCount
                            opacity: 0.7
                        }
                    }
                    Label {
                        text: modelData.kind + " · " + modelData.lastUsedLabel
                        font.pixelSize: 11
                        opacity: 0.6
                    }
                }
            }
        }

        Label {
            text: qsTr("Tool calls")
            font.pixelSize: 12
            opacity: 0.6
            Layout.topMargin: 8
        }

        Repeater {
            model: panel.latestAssistant && panel.latestAssistant.toolCalls
                  ? panel.latestAssistant.toolCalls : []

            ColumnLayout {
                required property var modelData

                Layout.fillWidth: true
                spacing: 0

                Label {
                    text: modelData.title + " · " + modelData.name
                    font.pixelSize: 12
                }
                Label {
                    text: modelData.resultPreview ? modelData.resultPreview : qsTr("running…")
                    font.pixelSize: 11
                    opacity: 0.6
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }
            }
        }
    }
}
