#include "PlatformServices.h"

#include <QClipboard>
#include <QGuiApplication>
#include <QStyleHints>
#include <QSysInfo>
#include <QtGlobal>

namespace sujiu::platform {

PlatformServices::PlatformServices(QObject *parent)
    : QObject(parent)
{
}

void PlatformServices::copyText(const QString &text)
{
    if (QClipboard *clipboard = QGuiApplication::clipboard()) {
        clipboard->setText(text);
    }
}

PlatformServices::Appearance PlatformServices::appearance() const
{
    return m_appearance;
}

void PlatformServices::setAppearance(Appearance appearance)
{
    if (m_appearance == appearance) {
        return;
    }
    m_appearance = appearance;
    emit appearanceChanged();
}

bool PlatformServices::systemPrefersDark() const
{
    return QGuiApplication::styleHints()->colorScheme() == Qt::ColorScheme::Dark;
}

QString PlatformServices::platformSummary() const
{
    return QStringLiteral("Qt %1 · %2 %3")
        .arg(QString::fromLatin1(qVersion()), QSysInfo::prettyProductName(), QSysInfo::currentCpuArchitecture());
}

} // namespace sujiu::platform
