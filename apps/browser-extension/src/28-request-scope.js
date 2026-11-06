    // Validate the marked GenerateContent request without rewriting it.
    // Summaries need the ENTIRE conversation, including recent turns, so
    // the snapshot reflects the actual latest scene. The keepRounds setting
    // controls subsequent deletion only; it never trims the model input.
    const RequestScope = Object.freeze({
        prepare(bodyText, marker, keepRounds) {
            if (typeof bodyText !== "string" || !marker || !bodyText.includes(marker)) {
                return null;
            }
            const payload = safeParseJSON(bodyText);
            if (!Array.isArray(payload) || !Array.isArray(payload[1])) {
                throw new Error("Google 请求历史格式无法验证，拒绝本次摘要");
            }
            const entries = payload[1];
            if (entries.length < 4 || entries.some(item =>
                !Array.isArray(item) ||
                !["user", "model"].includes(item[1]) ||
                !Array.isArray(item[0]))) {
                throw new Error("请求包含无法识别的历史轮次，停止摘要");
            }
            const last = entries[entries.length - 1];
            if (last[1] !== "user" || !JSON.stringify(last).includes(marker)) {
                throw new Error("无法确认请求末尾是本次摘要指令");
            }
            const original = entries.slice(0, -1);
            const split = RoundSplitter.partition(original.map(entry => ({
                role: entry[1], entry
            })), keepRounds);
            if (split.recentRounds !== keepRounds || split.olderRounds < 1) {
                throw new Error("无法准确计算将来清理时的保留轮数");
            }
            // The request body is untouched. Both older and protected recent
            // turns are visible to the summarizing model; only the cleanup
            // operation selects which original turns to delete.
            return {
                bodyText,
                olderRounds: split.olderRounds,
                recentRounds: split.recentRounds,
                totalRounds: split.totalRounds,
                transmittedRounds: split.totalRounds
            };
        }
    });
