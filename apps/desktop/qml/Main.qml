import QtQuick
import QtQuick.Controls

// Root window. It owns the visual system and page navigation only; all
// conversation data and state come from the presentation controller `App`.
ApplicationWindow {
    id: window

    visible: true
    width: 1180
    height: 780
    minimumWidth: 380
    minimumHeight: 480
    title: "Sujiu"

    readonly property bool wide: width >= 900

    palette.window: App.darkTheme ? "#15171b" : "#f6f6f7"
    palette.windowText: App.darkTheme ? "#e8e9ec" : "#1b1c1f"
    palette.base: App.darkTheme ? "#1d1f24" : "#ffffff"
    palette.alternateBase: App.darkTheme ? "#23262c" : "#f0f0f2"
    palette.text: App.darkTheme ? "#e8e9ec" : "#1b1c1f"
    palette.placeholderText: App.darkTheme ? "#8b8f98" : "#7a7e87"
    palette.button: App.darkTheme ? "#252830" : "#ececed"
    palette.buttonText: App.darkTheme ? "#e8e9ec" : "#1b1c1f"
    palette.highlight: "#4c6ef5"
    palette.highlightedText: "#ffffff"
    palette.mid: App.darkTheme ? "#3a3f49" : "#c9cbd1"

    // Page navigation is a view concern: the same chat state is reused by every
    // destination, and the wide layout is only a layout promotion of this one.
    StackView {
        id: pages

        anchors.fill: parent
        initialItem: ChatShell {
            onSettingsRequested: pages.push(settingsPage)
            onCharacterLibraryRequested: pages.push(libraryPage)
            onCharacterDetailRequested: (characterId) => pages.push(detailPage, { characterId: characterId })
        }

        Component {
            id: settingsPage
            SettingsPage {
                onBackRequested: pages.pop()
            }
        }

        Component {
            id: libraryPage
            CharacterLibraryPage {
                onBackRequested: pages.pop()
                onCharacterSelected: (characterId) => pages.push(detailPage, { characterId: characterId })
            }
        }

        Component {
            id: detailPage
            CharacterDetailPage {
                onBackRequested: pages.pop()
            }
        }
    }
}
