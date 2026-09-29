#pragma once

#include <QObject>
#include <QString>

namespace sujiu::platform {

/// Every desktop platform capability lives behind this object.
///
/// Pages never touch `QClipboard`, `QSysInfo` or `QGuiApplication` directly, so
/// a layout rewrite cannot drag platform behaviour along with it.
class PlatformServices : public QObject
{
    Q_OBJECT

public:
    enum class Appearance { System, Light, Dark };
    Q_ENUM(Appearance)

    explicit PlatformServices(QObject *parent = nullptr);

    void copyText(const QString &text);
    Appearance appearance() const;
    void setAppearance(Appearance appearance);
    bool systemPrefersDark() const;
    QString platformSummary() const;

signals:
    void appearanceChanged();

private:
    Appearance m_appearance = Appearance::System;
};

} // namespace sujiu::platform
