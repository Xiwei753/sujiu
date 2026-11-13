    const CompactionTask = Object.freeze({
        markerPattern: /\[\[SUJIU_SCOPE_V2:([a-z0-9-]{8,80})\]\]/i,
        storagePrefix: "sujiu:aistudio:summary-task:v2:",
        memoryPattern: /\[\[SUJIU_MEMORY_V1:([a-z0-9-]{8,80})\]\]/gi,
        memoryMarker(marker) {
            const match = typeof marker === "string" ? marker.match(this.markerPattern) : null;
            if (!match) throw new Error("摘要任务标识无效");
            return "[[SUJIU_MEMORY_V1:" + match[1] + "]]";
        },
        getMemoryMarkers(text) {
            if (typeof text !== "string") return [];
            return [...text.matchAll(this.memoryPattern)].map(match => match[0]);
        },
        isPinnedSummary(text) {
            return this.isProtectedMessage(text);
        },
        // The user-facing protection marker is the five letters "sujiu".
        // Older saved model summaries with SUJIU_MEMORY_V1 keep working too.
        // Callers must exclude native Thinking and exact SUJIU_SCOPE_V2
        // USER tasks before applying this general text predicate.
        isProtectedMessage(text) {
            return typeof text === "string" &&
                text.toLowerCase().includes("sujiu");
        },
        create(promptId, keepRounds) {
            if (!promptId || !Number.isSafeInteger(keepRounds) || keepRounds < 1 || keepRounds > 100) {
                throw new Error("无效的会话 ID 或保留轮数");
            }
            const id = typeof globalThis.crypto?.randomUUID === "function"
                ? globalThis.crypto.randomUUID()
                : Date.now().toString(36) + "-" + Math.random().toString(36).slice(2);
            return {
                promptId,
                id,
                marker: "[[SUJIU_SCOPE_V2:" + id + "]]",
                keepRounds,
                createdAt: Date.now()
            };
        },
        compose(basePrompt, task) {
            // Only old turns are carried in the scoped request. Protected
            // recent-turn counts must never be included in the model input.
            return basePrompt.trim() +
                "\n\n请仅依据本次请求实际提供的历史记录整理摘要。不要推测后续剧情。" +
                "\n\n摘要第一行单独输出这五个字母：sujiu" +
                "\n第二行原样输出任务记忆标识：" + this.memoryMarker(task.marker) +
                "\n然后继续输出完整的摘要正文。以上两行会随模型摘要一起保留。" +
                "\n\n[客户端任务] " + task.marker;
        },
        parsePrompt(prompt) {
            if (typeof prompt !== "string") return null;
            const match = prompt.match(this.markerPattern);
            return match ? { marker: match[0] } : null;
        },
        save(task) {
            try { localStorage.setItem(this.storagePrefix + task.promptId, JSON.stringify(task)); }
            catch {}
        },
        confirmFullHistory(promptId, marker, stats) {
            const previous = this.read(promptId);
            if (!previous || previous.marker !== marker ||
                stats?.recentRounds !== previous.keepRounds ||
                stats?.transmittedRounds !== stats?.totalRounds ||
                !Number.isSafeInteger(stats.olderRounds) || stats.olderRounds < 1) {
                throw new Error("摘要请求未包含全部历史，或保留轮数不一致");
            }
            // The model saw every original turn. keepRounds is deletion-only.
            this.save({ ...previous, fullHistoryVerified: true,
                olderRounds: stats.olderRounds, totalRounds: stats.totalRounds });
        },
        read(promptId) {
            try {
                const raw = localStorage.getItem(this.storagePrefix + promptId);
                const task = raw ? JSON.parse(raw) : null;
                if (task?.promptId !== promptId || typeof task.marker !== "string" ||
                    !Number.isSafeInteger(task.keepRounds)) return null;
                return task;
            } catch { return null; }
        }
    });
