    // Restrict ONLY the marked summary GenerateContent request. Normal
    // conversations are sent through unchanged. Never silently fall back to
    // sending full history when the private RPC schema cannot be verified.
    const RequestScope = Object.freeze({
        prepare(bodyText, marker, keepRounds) {
            if (typeof bodyText !== "string" || !marker || !bodyText.includes(marker)) {
                return null;
            }
            const payload = safeParseJSON(bodyText);
            if (!Array.isArray(payload) || !Array.isArray(payload[1])) {
                throw new Error("Google 请求历史格式无法验证，已阻止发送完整上下文");
            }
            const entries = payload[1];
            if (entries.length < 4 || entries.some(item =>
                !Array.isArray(item) ||
                !["user", "model"].includes(item[1]) ||
                !Array.isArray(item[0]))) {
                throw new Error("请求含不兼容或多模态轮次，已停止压缩");
            }
            const last = entries[entries.length - 1];
            if (last[1] !== "user" || !JSON.stringify(last).includes(marker)) {
                throw new Error("无法确认请求末尾是本次压缩指令");
            }
            const original = entries.slice(0, -1);
            const split = RoundSplitter.partition(original.map(entry => ({
                role: entry[1], entry
            })), keepRounds);
            if (!split.olderRounds || split.recentRounds !== keepRounds) {
                throw new Error("旧历史与保留范围无法严格划分");
            }
            const old = split.older.map(turn => turn.entry);
            const recent = split.recent.map(turn => turn.entry);
            // The request is reconstructed from the original RPC payload with
            // the full original serialized old turns and one summary prompt.
            // Never send any protected recent turn entries in this request.
            const scoped = payload.slice();
            scoped[1] = [...old, last];
            const serialized = JSON.stringify(scoped);
            if (serialized === bodyText) {
                throw new Error("摘要请求没有真正缩减上下文，已拒绝发送");
            }
            // Guard against redundant copies of protected full turns outside
            // payload[1]. Google may change its wire schema in the future.
            const remainingFields = JSON.stringify(scoped.filter((_, i) => i !== 1));
            for (const entry of recent) {
                const text = JSON.stringify(entry[0]);
                if (text.length > 40 && remainingFields.includes(text)) {
                    throw new Error("近期内容存在于请求的其他字段，已停止发送以防泄露到摘要上下文");
                }
            }
            return {
                bodyText: serialized,
                olderRounds: split.olderRounds,
                recentRounds: split.recentRounds,
                totalRounds: split.totalRounds
            };
        }
    });
