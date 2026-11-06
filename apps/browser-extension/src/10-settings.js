    function loadUserSettings() {
        try {
            const raw = localStorage.getItem(STORAGE_KEY);
            if (!raw) {
                return { ...DEFAULT_SETTINGS };
            }
            const parsed = JSON.parse(raw);
            if (parsed && typeof parsed === "object") {
                return { ...DEFAULT_SETTINGS, ...parsed };
            }
            return { ...DEFAULT_SETTINGS };
        } catch (err) {
            console.warn(Config.TAGS.NET, "Failed to load compressor settings", err);
            return { ...DEFAULT_SETTINGS };
        }
    }

    function persistUserSettings() {
        try {
            localStorage.setItem(STORAGE_KEY, JSON.stringify(userSettings));
        } catch (err) {
            console.warn(Config.TAGS.NET, "Failed to store compressor settings", err);
        }
    }

    function updateUserSettings(partial) {
        userSettings = { ...userSettings, ...partial };
        persistUserSettings();
    }

    function getCompressPrompt() {
        return userSettings?.compressPrompt || Config.PROMPTS.FULL;
    }

    function getSnapshotRegex() {
        return (userSettings?.snapshotRegex || "").trim();
    }

    function getTailRetentionConfig() {
        const percent = Number(userSettings?.tailPercent);
        const minChars = Number(userSettings?.tailMinChars);
        const normalizedPercent = Number.isFinite(percent) ? Math.min(Math.max(percent, 0), 100) : DEFAULT_SETTINGS.tailPercent;
        const normalizedMinChars = Number.isFinite(minChars) && minChars > 0 ? Math.floor(minChars) : DEFAULT_SETTINGS.tailMinChars;
        return { percent: normalizedPercent, minChars: normalizedMinChars };
    }

    function deriveSnippet(prompt) {
        const text = (prompt || "").trim();
        if (!text) {
            return Config.PROMPTS.SNIPPET;
        }
        return text.slice(0, 120);
    }

    function refreshActivePromptFromSettings() {
        state.activeCompressionPrompt = getCompressPrompt();
        state.activeCompressionSnippet = deriveSnippet(state.activeCompressionPrompt);
    }

    function getSettingsOrigin() {
        const url = Config.SETTINGS?.PAGE_URL;
        if (!url) return null;
        try {
            return new URL(url, location.href).origin;
        } catch {
            return null;
        }
    }

    function getSettingsSnapshot() {
        return {
            compressPrompt: userSettings?.compressPrompt || Config.PROMPTS.FULL,
            snapshotRegex: userSettings?.snapshotRegex || "",
            tailPercent: Number.isFinite(userSettings?.tailPercent) ? userSettings.tailPercent : DEFAULT_SETTINGS.tailPercent,
            tailMinChars: Number.isFinite(userSettings?.tailMinChars) ? userSettings.tailMinChars : DEFAULT_SETTINGS.tailMinChars
        };
    }

    function normalizeSettingsPayload(payload) {
        if (!payload || typeof payload !== "object") {
            return { ...DEFAULT_SETTINGS };
        }
        const next = { ...DEFAULT_SETTINGS };
        if (typeof payload.compressPrompt === "string") {
            const trimmed = payload.compressPrompt.trim();
            next.compressPrompt = trimmed || Config.PROMPTS.FULL;
        }
        if (typeof payload.snapshotRegex === "string") {
            next.snapshotRegex = payload.snapshotRegex.trim();
        }
        if (payload.tailPercent != null) {
            const parsedPercent = Number(payload.tailPercent);
            if (Number.isFinite(parsedPercent)) {
                next.tailPercent = Math.min(Math.max(parsedPercent, 0), 100);
            }
        }
        if (payload.tailMinChars != null) {
            const parsedMinChars = Number(payload.tailMinChars);
            if (Number.isFinite(parsedMinChars) && parsedMinChars >= 0) {
                next.tailMinChars = Math.floor(parsedMinChars);
            }
        }
        return next;
    }

    function scheduleSettingsHydration(win) {
        if (!win) return;
        const targetOrigin = settingsOrigin || "*";
        const message = {
            source: "aistudio-compressor",
            type: "hydrate",
            payload: getSettingsSnapshot()
        };
        let attempts = 0;
        const maxAttempts = 10;
        let timer = null;

        const dispatch = () => {
            if (!win || win.closed) {
                clearInterval(timer);
                return;
            }
            attempts += 1;
            try {
                win.postMessage(message, targetOrigin);
                log.ui("Sent settings hydration message", { attempts });
            } catch (err) {
                log.ui("Failed to send settings hydration message", err);
            }
            if (attempts >= maxAttempts) {
                clearInterval(timer);
            }
        };

        dispatch();
        timer = setInterval(dispatch, 1200);
    }

    function installSettingsMessageBridge() {
        window.addEventListener("message", handleSettingsMessage);
    }

    function handleSettingsMessage(event) {
        if (!event || !event.data) return;
        if (settingsOrigin && event.origin !== settingsOrigin) {
            return;
        }
        const data = event.data;
        if (data?.source !== "aistudio-compressor") {
            return;
        }
        if (data.type === "update-settings" && data.payload) {
            const normalized = normalizeSettingsPayload(data.payload);
            updateUserSettings(normalized);
            if (!state.compressionInProgress) {
                refreshActivePromptFromSettings();
            }
            log.ui("Settings updated via external page");
        }
    }

    function openSettingsWindow() {
        const targetUrl = Config.SETTINGS?.PAGE_URL;
        if (!targetUrl) {
            alert("未配置外部设置页面 URL，请更新脚本以启用设置界面。");
            return;
        }
        const win = window.open(targetUrl, "aistudio_compressor_settings", "width=720,height=720,resizable=yes,scrollbars=yes");
        if (!win || win.closed) {
            alert("无法打开设置窗口，请允许弹窗或在目标标签页手动执行脚本设置。");
            return;
        }
        scheduleSettingsHydration(win);
    }

    function resetSettingsToDefault() {
        if (!confirm("确认恢复 Compressed Prompt 及相关设置为默认值？")) {
            return;
        }
        userSettings = { ...DEFAULT_SETTINGS };
        persistUserSettings();
        if (!state.compressionInProgress) {
            refreshActivePromptFromSettings();
        }
        alert("已恢复默认设置。");
    }

    function registerMenuCommands() {
        if (typeof GM_registerMenuCommand !== "function") {
            return;
        }
        GM_registerMenuCommand("打开 Compressor 设置...", openSettingsWindow);
        GM_registerMenuCommand("重置 Compressor 设置", resetSettingsToDefault);
    }

    // ---------------------------
    // Event bridge & handlers
    // ---------------------------

    function installTopLevelHandlers() {
        if (!env.topWindow.__aistudio_compressor_handlers_installed) {
            env.topWindow.addEventListener(Config.EVENTS.REQUEST, Compression.handleRequestEvent);
            env.topWindow.addEventListener(Config.EVENTS.RESPONSE, Compression.handleResponseEvent);
            env.topWindow.__aistudio_compressor_handlers_installed = true;
        }

        if (!env.topWindow.__aistudio_compressor_request_logger) {
            env.topWindow.addEventListener(Config.EVENTS.REQUEST, event => {
                const detail = event?.detail || {};
                log.net("GenerateContent request observed", { url: detail.url });
            });
            env.topWindow.__aistudio_compressor_request_logger = true;
        }
    }

    function emitNetworkEvent(name, detail) {
        try {
            env.topWindow.dispatchEvent(new CustomEvent(name, { detail }));
        } catch (err) {
            log.net("Failed to dispatch network event", name, err);
        }
    }

    // ---------------------------
    // Network monitor module
    // ---------------------------
