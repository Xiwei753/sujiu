    // Summary markers are metadata in the user message. They let us match a
    // saved summary even if AI Studio adds thought entries after the reply.
    // No prompt contents, API keys or conversation text are stored locally.
    const CompactionTask = Object.freeze({
        markerPattern: /\[\[SUJIU_SUMMARY_V1:([a-z0-9-]{8,80}):KEEP=(\d{1,3})\]\]/i,
        legacyPattern: /Only summarize the completed dialogue BEFORE the last\s+(\d{1,3})\s+full user\/model exchanges\./i,
        storagePrefix: "sujiu:aistudio:summary-task:v1:",
        create(promptId, keepRounds) {
            if (!promptId || !Number.isSafeInteger(keepRounds) || keepRounds < 1 || keepRounds > 100) {
                throw new Error("无效的会话 ID 或保留轮数");
            }
            const id = typeof crypto?.randomUUID === "function"
                ? crypto.randomUUID()
                : Date.now().toString(36) + "-" + Math.random().toString(36).slice(2);
            return {
                promptId,
                id,
                marker: "[[SUJIU_SUMMARY_V1:" + id + ":KEEP=" + keepRounds + "]]",
                keepRounds,
                createdAt: Date.now()
            };
        },
        compose(basePrompt, task) {
            return basePrompt.trim() +
                "\n\n只总结最近 " + task.keepRounds + " 轮完整用户/模型对话之前的历史。" +
                "最近这些轮次由原聊天保留，用它们判断故事真正的最新状态。" +
                "\n\n【客户端任务标识】\n" + task.marker +
                "\n该标识用于匹配这次摘要与清理任务。";
        },
        parsePrompt(prompt) {
            if (typeof prompt !== "string") return null;
            const match = prompt.match(this.markerPattern);
            if (match) {
                const keepRounds = Number(match[2]);
                if (!Number.isSafeInteger(keepRounds) || keepRounds < 1 || keepRounds > 100) return null;
                return { marker: match[0], keepRounds, legacy: false };
            }
            const legacy = prompt.match(this.legacyPattern);
            if (!legacy) return null;
            const keepRounds = Number(legacy[1]);
            if (!Number.isSafeInteger(keepRounds) || keepRounds < 1 || keepRounds > 100) return null;
            return { marker: null, keepRounds, legacy: true };
        },
        save(task) {
            try {
                localStorage.setItem(this.storagePrefix + task.promptId, JSON.stringify(task));
            } catch {}
        },
        read(promptId) {
            try {
                const raw = localStorage.getItem(this.storagePrefix + promptId);
                if (!raw) return null;
                const task = JSON.parse(raw);
                if (task?.promptId !== promptId || typeof task.marker !== "string") return null;
                return task;
            } catch {
                return null;
            }
        }
    });
