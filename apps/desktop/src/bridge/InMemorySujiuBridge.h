#pragma once

#include "SujiuBridge.h"

#include <QHash>
#include <QTimer>

namespace sujiu::bridge {

/// Preview conversation source used until `sujiu-ffi` exposes the conversation
/// API described in `docs/UI_ARCHITECTURE.md`.
///
/// It exists so the presentation layer and the desktop shell can be built and
/// tested against the real contract. Replacing it with an FFI-backed
/// implementation must not change any QML or presentation file.
class InMemorySujiuBridge final : public SujiuBridge
{
    Q_OBJECT

public:
    explicit InMemorySujiuBridge(QObject *parent = nullptr);

    QVariantList sessions() const override;
    QVariantList characters(const QString &query) const override;
    QVariantList models() const override;
    QVariantList contextSources(const QString &sessionId) const override;
    QVariantList messages(const QString &sessionId) const override;

    void sendTurn(const QString &sessionId, const QString &input) override;
    void cancelTurn(const QString &sessionId) override;

private:
    void emitScript(int step, const QString &sessionId);
    void schedule(int delayMs);

    QHash<QString, QVariantList> m_messages;
    QHash<QString, bool> m_cancelled;
    QTimer m_timer;
    int m_step = 0;
    QString m_sessionId;
    int m_messageCounter = 0;
    int m_callCounter = 0;
};

} // namespace sujiu::bridge
