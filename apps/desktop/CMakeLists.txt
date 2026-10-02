cmake_minimum_required(VERSION 3.21)

project(SujiuDesktop VERSION 0.1 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Quick QuickControls2)
qt_standard_project_setup(REQUIRES 6.5)
qt_policy(SET QTP0004 NEW)

qt_add_executable(sujiu_desktop
    src/main.cpp
    src/bridge/InMemorySujiuBridge.cpp
    src/bridge/InMemorySujiuBridge.h
    src/bridge/SujiuBridge.h
    src/platform/PlatformServices.cpp
    src/platform/PlatformServices.h
    src/presentation/ChatController.cpp
    src/presentation/ChatController.h
)

qt_add_qml_module(sujiu_desktop
    URI Sujiu
    VERSION 1.0
    QML_FILES
        qml/Main.qml
        qml/ChatShell.qml
        qml/ChatTopBar.qml
        qml/ConversationView.qml
        qml/MessageItem.qml
        qml/Composer.qml
        qml/HistoryPanel.qml
        qml/ModelSelectorSheet.qml
        qml/ContextPanel.qml
        qml/SettingsPage.qml
        qml/AppearanceSettingsPage.qml
        qml/AboutSettingsPage.qml
        qml/CharacterLibraryPage.qml
        qml/CharacterDetailPage.qml
)

set_source_files_properties(qml/Main.qml PROPERTIES QT_QML_SOURCE_DIRECTORY qml)

target_link_libraries(sujiu_desktop PRIVATE Qt6::Quick Qt6::QuickControls2)
