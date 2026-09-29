#include "ChatController.h"

#include <QDateTime>
#include <QHash>
#include <QVariantMap>

using sujiu::bridge::SujiuBridge;
using sujiu::platform::PlatformServices;

ChatController::ChatController(SujiuBridge *bridge, PlatformServices *platform, QObject *parent)
    : QObject(parent)
    , m_bridge(bridge)
    , m_platform(platform)
{
    m_currentSessionId = QStringLiteral("session-1");
    m_currentCharacterId = QStringLiteral("char-lin");
    m_currentModelId = QStringLiteral("model-default");

    connect(m_bridge, &SujiuBridge::turnStarted, this, [this](const QString &sessionId) {
        if (sessionId != m_currentSessionId) {
            return;
        }
        setGenerationState(QStringLiteral("submitting"), QString(), QString());
    });

    connect(m_bridge, &SujiuBridge::textDelta, this, [this](const QString &sessionId, const QString &text) {
        if (sessionId != m_currentSessionId) {
            return;
        }
        setGenerationState(QStringLiteral("streaming"), QString(), QString());
    });

    connect(m_bridge, &SujiuBridge::toolCallStarted, this, [this](const QString &) {
        setGenerationState(QStringLiteral("executingTool"),
                           QStringLiteral("Running tool"),
                           QString());
    });

    connect(m_bridge, &SujiuBridge::toolCallFinished, this, [this](const QString &) {
        setGenerationState(QStringLiteral("continuingAfterTool"),
                           QStringLiteral("Reading tool result"),
                           QString());
    });

    connect(m_bridge, &SujiuBridge::turnCompleted, this, [this](const QString &) {
        setGenerationState(QStringLiteral("completed"), QString(), QString());
    });

    connect(m_bridge, &SujiuBridge::turnFailed, this, [this](const QString &, const QString &message) {
        setGenerationState(QStringLiteral("failed"), QString(), message);
    });

    connect(m_bridge, &SujiuBridge::turnCancelled, this, [this](const QString &) {
        setGenerationState(QStringLiteral("cancelled"), QString(), QString());
    });

    connect(m_platform, &PlatformServices::appearanceChanged, this, &ChatController::appearanceChanged);
}

QVariantList ChatController::sessionGroups() const
{
    return groupedSessions();
}

QVariantList ChatController::messages() const
{
    return m_bridge->messages(m_currentSessionId);
}

QVariantList ChatController::characters() const
{
    return sortedCharacters();
}

QVariantList ChatController::models() const
{
    return m_bridge->models();
}

QVariantList ChatController::contextSources() const
{
    return m_bridge->contextSources(m_currentSessionId);
}

QString ChatController::currentSessionId() const
{
    return m_currentSessionId;
}

QString ChatController::currentCharacterId() const
{
    return m_currentCharacterId;
}

QString ChatController::currentCharacterName() const
{
    for (const QVariant &entry : m_bridge->characters(QString())) {
        const QVariantMap character = entry.toMap();
        if (character.value(QStringLiteral("id")).toString() == m_currentCharacterId) {
            return character.value(QStringLiteral("name")).toString();
        }
    }
    return QString();
}

QString ChatController::currentCharacterTagline() const
{
    for (const QVariant &entry : m_bridge->characters(QString())) {
        const QVariantMap character = entry.toMap();
        if (character.value(QStringLiteral("id")).toString() == m_currentCharacterId) {
            return character.value(QStringLiteral("tagline")).toString();
        }
    }
    return QString();
}

QString ChatController::currentModelId() const
{
    return m_currentModelId;
}

QString ChatController::currentModelName() const
{
    for (const QVariant &entry : m_bridge->models()) {
        const QVariantMap model = entry.toMap();
        if (model.value(QStringLiteral("id")).toString() == m_currentModelId) {
            return model.value(QStringLiteral("name")).toString();
        }
    }
    return QString();
}

QString ChatController::draft() const
{
    return m_draft;
}

void ChatController::setDraft(const QString &draft)
{
    if (m_draft == draft) {
        return;
    }
    m_draft = draft;
    emit draftChanged();
}

QString ChatController::generationState() const
{
    return m_generationState;
}

QString ChatController::statusLabel() const
{
    return m_statusLabel;
}

QString ChatController::errorMessage() const
{
    return m_errorMessage;
}

bool ChatController::busy() const
{
    return m_generationState == QLatin1String("submitting") || m_generationState == QLatin1String("streaming")
        || m_generationState == QLatin1String("executingTool")
        || m_generationState == QLatin1String("continuingAfterTool");
}

QString ChatController::appearanceMode() const
{
    switch (m_platform->appearance()) {
    case PlatformServices::Appearance::Light:
        return QStringLiteral("light");
    case PlatformServices::Appearance::Dark:
        return QStringLiteral("dark");
    case PlatformServices::Appearance::System:
        break;
    }
    return QStringLiteral("system");
}

void ChatController::setAppearanceMode(const QString &mode)
{
    if (mode == QLatin1String("light")) {
        m_platform->setAppearance(PlatformServices::Appearance::Light);
    } else if (mode == QLatin1String("dark")) {
        m_platform->setAppearance(PlatformServices::Appearance::Dark);
    } else {
        m_platform->setAppearance(PlatformServices::Appearance::System);
    }
    emit appearanceChanged();
}

bool ChatController::darkTheme() const
{
    if (m_platform->appearance() == PlatformServices::Appearance::Light) {
        return false;
    }
    if (m_platform->appearance() == PlatformServices::Appearance::Dark) {
        return true;
    }
    return m_platform->systemPrefersDark();
}

QString ChatController::platformSummary() const
{
    return m_platform->platformSummary();
}

void ChatController::send()
{
    if (busy() || m_draft.trimmed().isEmpty()) {
        return;
    }

    const QString input = m_draft.trimmed();
    setDraft(QString());
    setGenerationState(QStringLiteral("submitting"), QString(), QString());
    m_bridge->sendTurn(m_currentSessionId, input);
}

void ChatController::cancel()
{
    if (!busy()) {
        return;
    }
    m_bridge->cancelTurn(m_currentSessionId);
}

void ChatController::openSession(const QString &sessionId)
{
    if (sessionId.isEmpty() || sessionId == m_currentSessionId) {
        return;
    }
    m_currentSessionId = sessionId;
    setGenerationState(QStringLiteral("idle"), QString(), QString());
    emit stateChanged();
}

void ChatController::newSession()
{
    m_currentSessionId = QStringLiteral("session-new");
    setGenerationState(QStringLiteral("idle"), QString(), QString());
}

void ChatController::selectModel(const QString &modelId)
{
    m_currentModelId = modelId;
    emit stateChanged();
}

void ChatController::selectCharacter(const QString &characterId)
{
    m_currentCharacterId = characterId;
    emit stateChanged();
}

void ChatController::searchCharacters(const QString &query)
{
    m_characterQuery = query;
    emit stateChanged();
}

void ChatController::copyMessage(const QString &text)
{
    m_platform->copyText(text);
}

void ChatController::setGenerationState(const QString &state,
                                        const QString &statusLabel,
                                        const QString &errorMessage)
{
    m_generationState = state;
    m_statusLabel = statusLabel;
    m_errorMessage = errorMessage;
    emit stateChanged();
}

QVariantList ChatController::groupedSessions() const
{
    const QDateTime today = QDateTime::currentDateTime();
    QStringList order;
    QHash<QString, QVariantList> buckets;

    const QVariantList sessions = m_bridge->sessions();
    for (const QVariant &entry : sessions) {
        const QVariantMap session = entry.toMap();
        const QDateTime updated = QDateTime::fromMSecsSinceEpoch(
            session.value(QStringLiteral("updatedAtMs")).toLongLong());
        const int days = updated.date().daysTo(today.date());

        QString label = QStringLiteral("Earlier");
        if (days <= 0) {
            label = QStringLiteral("Today");
        } else if (days == 1) {
            label = QStringLiteral("Yesterday");
        }

        if (!buckets.contains(label)) {
            order.append(label);
        }
        buckets[label].append(session);
    }

    QVariantList groups;
    for (const QString &label : order) {
        QVariantMap group;
        group.insert(QStringLiteral("label"), label);
        group.insert(QStringLiteral("sessions"), buckets.value(label));
        groups.append(group);
    }
    return groups;
}

QVariantList ChatController::sortedCharacters() const
{
    return m_bridge->characters(m_characterQuery);
}
