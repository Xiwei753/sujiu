#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQuickStyle>

#include "bridge/InMemorySujiuBridge.h"
#include "platform/PlatformServices.h"
#include "presentation/ChatController.h"

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);

    QQuickStyle::setStyle(QStringLiteral("Fusion"));

    sujiu::bridge::InMemorySujiuBridge bridge;
    sujiu::platform::PlatformServices platform;
    ChatController controller(&bridge, &platform);

    // Presentation state is the only thing QML may see from the lower layers.
    qmlRegisterSingletonInstance("Sujiu", 1, 0, "App", &controller);

    QQmlApplicationEngine engine;
    engine.loadFromModule("Sujiu", "Main");

    if (engine.rootObjects().isEmpty()) {
        return -1;
    }

    return app.exec();
}
