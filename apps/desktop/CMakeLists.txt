cmake_minimum_required(VERSION 3.21)

project(SujiuDesktop VERSION 0.1 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Quick)
qt_standard_project_setup(REQUIRES 6.5)

qt_add_executable(sujiu_desktop
    src/main.cpp
)

qt_add_qml_module(sujiu_desktop
    URI Sujiu
    VERSION 1.0
    QML_FILES
        qml/Main.qml
)

target_link_libraries(sujiu_desktop PRIVATE Qt6::Quick)
