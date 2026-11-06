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
            return this.getMemoryMarkers(text).length > 0;
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
                "\n\n摘要第一行必须独立、原样输出以下记忆标识：" + this.memoryMarker(task.marker) +
                "\n这是一条会被永久保留的压缩记忆，标识之后才是摘要正文。" +
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
