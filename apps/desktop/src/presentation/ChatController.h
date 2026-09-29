#pragma once

#include "../platform/PlatformServices.h"
#include "../bridge/SujiuBridge.h"

#include <QObject>
#include <QString>
#include <QVariantList>

/// Desktop presentation controller.
///
/// It turns UI intents into bridge calls and exposes view state to QML. It owns
/// page state only: it never owns navigation, animation, layout or raw FFI
/// details, and it never mutates provider payloads directly.
class ChatController : public QObject
{
    Q_OBJECT

    Q_PROPERTY(QVariantList sessionGroups READ sessionGroups NOTIFY stateChanged)
    Q_PROPERTY(QVariantList messages READ messages NOTIFY stateChanged)
    Q_PROPERTY(QVariantList characters READ characters NOTIFY stateChanged)
    Q_PROPERTY(QVariantList models READ models NOTIFY stateChanged)
    Q_PROPERTY(QVariantList contextSources READ contextSources NOTIFY stateChanged)
    Q_PROPERTY(QString currentSessionId READ currentSessionId NOTIFY stateChanged)
    Q_PROPERTY(QString currentCharacterId READ currentCharacterId NOTIFY stateChanged)
    Q_PROPERTY(QString currentCharacterName READ currentCharacterName NOTIFY stateChanged)
    Q_PROPERTY(QString currentCharacterTagline READ currentCharacterTagline NOTIFY stateChanged)
    Q_PROPERTY(QString currentModelId READ currentModelId NOTIFY stateChanged)
    Q_PROPERTY(QString currentModelName READ currentModelName NOTIFY stateChanged)
    Q_PROPERTY(QString draft READ draft WRITE setDraft NOTIFY draftChanged)
    Q_PROPERTY(QString generationState READ generationState NOTIFY stateChanged)
    Q_PROPERTY(QString statusLabel READ statusLabel NOTIFY stateChanged)
    Q_PROPERTY(QString errorMessage READ errorMessage NOTIFY stateChanged)
    Q_PROPERTY(bool busy READ busy NOTIFY stateChanged)
    Q_PROPERTY(QString appearanceMode READ appearanceMode WRITE setAppearanceMode NOTIFY appearanceChanged)
    Q_PROPERTY(bool darkTheme READ darkTheme NOTIFY appearanceChanged)
    Q_PROPERTY(QString platformSummary READ platformSummary NOTIFY stateChanged)

public:
    ChatController(sujiu::bridge::SujiuBridge *bridge,
                   sujiu::platform::PlatformServices *platform,
                   QObject *parent = nullptr);

    QVariantList sessionGroups() const;
    QVariantList messages() const;
    QVariantList characters() const;
    QVariantList models() const;
    QVariantList contextSources() const;

    QString currentSessionId() const;
    QString currentCharacterId() const;
    QString currentCharacterName() const;
    QString currentCharacterTagline() const;
    QString currentModelId() const;
    QString currentModelName() const;
    QString draft() const;
    void setDraft(const QString &draft);
    QString generationState() const;
    QString statusLabel() const;
    QString errorMessage() const;
    bool busy() const;

    QString appearanceMode() const;
    void setAppearanceMode(const QString &mode);
    bool darkTheme() const;
    QString platformSummary() const;

    Q_INVOKABLE void send();
    Q_INVOKABLE void cancel();
    Q_INVOKABLE void openSession(const QString &sessionId);
    Q_INVOKABLE void newSession();
    Q_INVOKABLE void selectModel(const QString &modelId);
    Q_INVOKABLE void selectCharacter(const QString &characterId);
    Q_INVOKABLE void searchCharacters(const QString &query);
    Q_INVOKABLE void copyMessage(const QString &text);

signals:
    void stateChanged();
    void draftChanged();
    void appearanceChanged();

private:
    void setGenerationState(const QString &state, const QString &statusLabel, const QString &errorMessage);
    QVariantList groupedSessions() const;
    QVariantList sortedCharacters() const;

    sujiu::bridge::SujiuBridge *m_bridge = nullptr;
    sujiu::platform::PlatformServices *m_platform = nullptr;

    QString m_currentSessionId;
    QString m_currentCharacterId;
    QString m_currentModelId;
    QString m_characterQuery;
    QString m_draft;
    QString m_generationState = QStringLiteral("idle");
    QString m_statusLabel;
    QString m_errorMessage;
};
