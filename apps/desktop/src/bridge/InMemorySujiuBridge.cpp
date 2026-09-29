#include "InMemorySujiuBridge.h"

#include <QDateTime>
#include <QVariantMap>

namespace sujiu::bridge {
namespace {

QVariantMap makeSession(const QString &id, const QString &title, int messageCount, qint64 minutesAgo)
{
    QVariantMap session;
    session.insert(QStringLiteral("id"), id);
    session.insert(QStringLiteral("title"), title);
    session.insert(QStringLiteral("messageCount"), messageCount);
    session.insert(
        QStringLiteral("updatedAtMs"),
        QDateTime::currentDateTime().addSecs(-minutesAgo * 60).toMSecsSinceEpoch());
    return session;
}

QVariantMap makeCharacter(const QString &id,
                          const QString &name,
                          const QString &tagline,
                          const QString &greeting,
                          int recentOrder)
{
    QVariantMap character;
    character.insert(QStringLiteral("id"), id);
    character.insert(QStringLiteral("name"), name);
    character.insert(QStringLiteral("tagline"), tagline);
    character.insert(QStringLiteral("greeting"), greeting);
    character.insert(QStringLiteral("recentOrder"), recentOrder);
    return character;
}

QVariantMap makeModel(const QString &id,
                      const QString &name,
                      const QString &providerLabel,
                      const QStringList &capabilities,
                      bool available)
{
    QVariantMap model;
    model.insert(QStringLiteral("id"), id);
    model.insert(QStringLiteral("name"), name);
    model.insert(QStringLiteral("providerLabel"), providerLabel);
    model.insert(QStringLiteral("capabilities"), capabilities);
    model.insert(QStringLiteral("available"), available);
    return model;
}

QVariantMap makeSource(const QString &id,
                       const QString &kind,
                       const QString &label,
                       int recordCount,
                       const QString &lastUsedLabel)
{
    QVariantMap source;
    source.insert(QStringLiteral("id"), id);
    source.insert(QStringLiteral("kind"), kind);
    source.insert(QStringLiteral("label"), label);
    source.insert(QStringLiteral("recordCount"), recordCount);
    source.insert(QStringLiteral("lastUsedLabel"), lastUsedLabel);
    return source;
}

QVariantMap makeMessage(const QString &id, const QString &role, const QString &text)
{
    QVariantMap message;
    message.insert(QStringLiteral("id"), id);
    message.insert(QStringLiteral("role"), role);
    message.insert(QStringLiteral("text"), text);
    message.insert(QStringLiteral("streaming"), false);
    message.insert(QStringLiteral("toolCalls"), QVariantList());
    message.insert(QStringLiteral("error"), QString());
    return message;
}

} // namespace

InMemorySujiuBridge::InMemorySujiuBridge(QObject *parent)
    : SujiuBridge(parent)
{
    m_messages.insert(QStringLiteral("session-1"),
                      QVariantList{
                          makeMessage(QStringLiteral("m-1"),
                                      QStringLiteral("user"),
                                      QStringLiteral("Lantern lighting rules for this street?")),
                          makeMessage(QStringLiteral("m-2"),
                                      QStringLiteral("assistant"),
                                      QStringLiteral("Oil lamps stay lit until the third bell. "
                                                     "The east gate is doused at midnight.")),
                      });
}

QVariantList InMemorySujiuBridge::sessions() const
{
    return QVariantList{
        makeSession(QStringLiteral("session-1"), QStringLiteral("Lantern street"), 12, 3),
        makeSession(QStringLiteral("session-2"), QStringLiteral("Archive questions"), 30, 190),
        makeSession(QStringLiteral("session-3"), QStringLiteral("Old harbour run"), 8, 1500),
        makeSession(QStringLiteral("session-4"), QStringLiteral("Winter customs"), 4, 4000),
    };
}

QVariantList InMemorySujiuBridge::characters(const QString &query) const
{
    QVariantList all{
        makeCharacter(QStringLiteral("char-lin"),
                      QStringLiteral("Lin"),
                      QStringLiteral("Night archivist"),
                      QStringLiteral("*The archive door closes at the second bell.*"),
                      0),
        makeCharacter(QStringLiteral("char-yan"),
                      QStringLiteral("Yan"),
                      QStringLiteral("Harbour pilot"),
                      QStringLiteral("*Ropes are coiled before the fog returns.*"),
                      1),
        makeCharacter(QStringLiteral("char-shu"),
                      QStringLiteral("Shu"),
                      QStringLiteral("Tea seller, town square"),
                      QStringLiteral("*Steam rises from the second pot.*"),
                      2),
        makeCharacter(QStringLiteral("char-qiao"),
                      QStringLiteral("Qiao"),
                      QStringLiteral("Courier of the north road"),
                      QStringLiteral("*A wax-sealed letter is pushed under the door.*"),
                      3),
    };

    if (query.trimmed().isEmpty()) {
        return all;
    }

    QVariantList filtered;
    for (const QVariant &entry : all) {
        const QVariantMap character = entry.toMap();
        if (character.value(QStringLiteral("name")).toString().contains(query, Qt::CaseInsensitive)
            || character.value(QStringLiteral("tagline")).toString().contains(query, Qt::CaseInsensitive)) {
            filtered.append(entry);
        }
    }
    return filtered;
}

QVariantList InMemorySujiuBridge::models() const
{
    return QVariantList{
        makeModel(QStringLiteral("model-default"),
                  QStringLiteral("Sujiu Chat"),
                  QStringLiteral("OpenAI compatible"),
                  QStringList{QStringLiteral("chat"), QStringLiteral("tools"), QStringLiteral("stream")},
                  true),
        makeModel(QStringLiteral("model-long"),
                  QStringLiteral("Sujiu Chat Long"),
                  QStringLiteral("OpenAI compatible"),
                  QStringList{QStringLiteral("chat"), QStringLiteral("tools"), QStringLiteral("stream"),
                               QStringLiteral("long context")},
                  true),
        makeModel(QStringLiteral("model-local"),
                  QStringLiteral("Local Draft"),
                  QStringLiteral("OpenAI compatible"),
                  QStringList{QStringLiteral("chat")},
                  false),
    };
}

QVariantList InMemorySujiuBridge::contextSources(const QString &sessionId) const
{
    Q_UNUSED(sessionId)

    return QVariantList{
        makeSource(QStringLiteral("src-character"),
                   QStringLiteral("character"),
                   QStringLiteral("Character"),
                   1,
                   QStringLiteral("this turn")),
        makeSource(QStringLiteral("src-world"),
                   QStringLiteral("world_lore"),
                   QStringLiteral("World book"),
                   42,
                   QStringLiteral("this turn")),
        makeSource(QStringLiteral("src-story"),
                   QStringLiteral("story_event"),
                   QStringLiteral("Plot memory"),
                   18,
                   QStringLiteral("this turn")),
        makeSource(QStringLiteral("src-history"),
                   QStringLiteral("chat_history"),
                   QStringLiteral("Old chat"),
                   96,
                   QStringLiteral("2 records")),
    };
}

QVariantList InMemorySujiuBridge::messages(const QString &sessionId) const
{
    return m_messages.value(sessionId);
}

void InMemorySujiuBridge::sendTurn(const QString &sessionId, const QString &input)
{
    m_sessionId = sessionId;
    m_cancelled.insert(sessionId, false);
    m_step = 0;

    QVariantList &conversation = m_messages[sessionId];
    conversation.append(
        makeMessage(QStringLiteral("m-u-%1").arg(++m_messageCounter), QStringLiteral("user"), input));

    m_timer.stop();
    connect(&m_timer, &QTimer::timeout, this, [this] {
        const int step = m_step++;
        emitScript(step, m_sessionId);
    });
    emit turnStarted(sessionId);
    schedule(0);
}

void InMemorySujiuBridge::cancelTurn(const QString &sessionId)
{
    m_cancelled.insert(sessionId, true);
    m_timer.stop();
    if (m_sessionId == sessionId) {
        emit turnCancelled(sessionId);
    }
}

void InMemorySujiuBridge::schedule(int delayMs)
{
    m_timer.start(delayMs);
}

void InMemorySujiuBridge::emitScript(int step, const QString &sessionId)
{
    if (m_cancelled.value(sessionId)) {
        return;
    }

    const QString messageId = QStringLiteral("m-a-%1").arg(m_messageCounter);

    switch (step) {
    case 0: {
        QVariantList &conversation = m_messages[sessionId];
        conversation.append(makeMessage(messageId, QStringLiteral("assistant"), QString()));
        QVariantMap streaming = conversation.last().toMap();
        streaming.insert(QStringLiteral("streaming"), true);
        conversation[conversation.size() - 1] = streaming;
        emit textDelta(sessionId, QStringLiteral("Let me check the harbour ledger. "));
        break;
    }
    case 1:
        emit toolCallStarted(sessionId,
                             QStringLiteral("call-%1").arg(++m_callCounter),
                             QStringLiteral("search_context"),
                             QStringLiteral("Search context"),
                             QStringLiteral("{\"query\": \"lantern rules\"}"));
        break;
    case 2:
        emit toolCallFinished(sessionId,
                              QStringLiteral("call-%1").arg(m_callCounter),
                              QStringLiteral("search_context"),
                              false,
                              QStringLiteral("2 records · world_lore, chat_history"));
        break;
    case 3:
        emit textDelta(sessionId,
                      QStringLiteral("Oil lamps stay lit until the third bell; the east gate is "
                                     "doused at midnight."));
        break;
    case 4: {
        QVariantList &conversation = m_messages[sessionId];
        for (int index = 0; index < conversation.size(); ++index) {
            QVariantMap message = conversation.at(index).toMap();
            if (message.value(QStringLiteral("id")).toString() == messageId) {
                message.insert(QStringLiteral("streaming"), false);
                conversation[index] = message;
                break;
            }
        }
        emit turnCompleted(sessionId);
        return;
    }
    default:
        emit turnFailed(sessionId, QStringLiteral("Preview script exhausted"));
        return;
    }

    schedule(320);
}

} // namespace sujiu::bridge
