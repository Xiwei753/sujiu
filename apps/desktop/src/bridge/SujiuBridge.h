#pragma once

#include <QObject>
#include <QString>
#include <QVariantList>

namespace sujiu::bridge {

/// Provider-neutral conversation boundary for the desktop frontend.
///
/// The FFI-backed implementation owns every `sujiu-ffi` call and normalizes
/// Rust output into the coarse application events below. Nothing above this
/// boundary may depend on a provider wire format.
///
/// The signals mirror the normalized turn events described in
/// `docs/UI_ARCHITECTURE.md`:
/// `turn_started`, `text_delta`, `tool_call_started`, `tool_call_finished`,
/// `turn_completed`, `turn_failed`, `turn_cancelled`.
class SujiuBridge : public QObject
{
    Q_OBJECT

public:
    explicit SujiuBridge(QObject *parent = nullptr)
        : QObject(parent)
    {
    }

    ~SujiuBridge() override = default;

    virtual QVariantList sessions() const = 0;
    virtual QVariantList characters(const QString &query) const = 0;
    virtual QVariantList models() const = 0;
    virtual QVariantList contextSources(const QString &sessionId) const = 0;
    virtual QVariantList messages(const QString &sessionId) const = 0;

    virtual void sendTurn(const QString &sessionId, const QString &input) = 0;
    virtual void cancelTurn(const QString &sessionId) = 0;

signals:
    void turnStarted(const QString &sessionId);
    void textDelta(const QString &sessionId, const QString &text);
    void toolCallStarted(const QString &sessionId,
                         const QString &callId,
                         const QString &name,
                         const QString &title,
                         const QString &argumentsPreview);
    void toolCallFinished(const QString &sessionId,
                          const QString &callId,
                          const QString &name,
                          bool isError,
                          const QString &resultPreview);
    void turnCompleted(const QString &sessionId);
    void turnFailed(const QString &sessionId, const QString &message);
    void turnCancelled(const QString &sessionId);
};

} // namespace sujiu::bridge
