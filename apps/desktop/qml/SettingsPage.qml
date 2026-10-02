import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Settings, first level: a category list and nothing else.
//
// `docs/UI_ARCHITECTURE.md` §1.9 defines the first level as destinations, with
// anything larger than a single screen on its own page behind one of them. The
// previous version laid the appearance choice and the about text out on one
// scrolling page, so the screen had no structure and nowhere to grow into.
//
// Only categories this frontend can open are listed. Provider & API and
// Diagnostics are in the spec, and the desktop bridge exposes neither an
// endpoint configuration nor a diagnostics log, so a row for them would be a
// button that leads nowhere — apps/desktop/TODO.md records the missing
// capability rather than this file inventing a page for it.
Page {
    id: page

    signal backRequested()

    readonly property var categories: [
        { key: "appearance", label: qsTr("Appearance") },
        { key: "about", label: qsTr("About") }
    ]

    header: ToolBar {
        RowLayout {
            anchors.fill: parent

            ToolButton {
                text: qsTr("Back")
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

    // A category push replaces the list rather than covering it, which is how a
    // settings window behaves: the back control returns to the list.
    StackView {
        id: pages

        anchors.fill: parent
        initialItem: categoryList

        Component {
            id: categoryList

            ListView {
                contentWidth: width
                clip: true
                spacing: 0

                model: page.categories

                delegate: ItemDelegate {
                    required property var modelData

                    width: ListView.view.width
                    text: modelData.label
                    onClicked: pages.push(page.componentFor(modelData.key))
                }
            }
        }
    }

    function componentFor(key) {
        return key === "appearance" ? appearancePage : aboutPage;
    }

    Component {
        id: appearancePage

        AppearanceSettingsPage {
            onBackRequested: pages.pop()
        }
    }

    Component {
        id: aboutPage

        AboutSettingsPage {
            onBackRequested: pages.pop()
        }
    }
}
