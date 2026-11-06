// ==UserScript==
// @name         Sujiu AI Studio Tools
// @namespace    https://github.com/Xiwei753/sujiu
// @version      0.3.2
// @description  AI Studio prompt presets and manual context compaction
// @match        https://aistudio.google.com/*
// @noframes
// @sandbox      raw
// @run-at       document-start
// @grant        none
// ==/UserScript==
(function () {
"use strict";
const bootstrapStatus = { version: "0.3.2", phase: "loaded", loadedAt: new Date().toISOString() };
window.__SUJIU_AISTUDIO_BOOT__ = bootstrapStatus;
console.info("[Sujiu AI Studio] injected", bootstrapStatus.version, location.href);
try {
const unsafeWindow = window;
// Adapted from lxchx/aistudio-compressor v0.3.2 (MIT); see THIRD_PARTY_NOTICES.md.
    const Config = {
        TAGS: {
            UI: "[Compressor UI]",
            NET: "[Compressor Monitor]"
        },
        TARGETS: {
            GENERATE: "/$rpc/google.internal.alkali.applications.makersuite.v1.MakerSuiteService/GenerateContent",
            RESOLVE: "/$rpc/google.internal.alkali.applications.makersuite.v1.MakerSuiteService/ResolveDriveResource",
            UPDATE: "/$rpc/google.internal.alkali.applications.makersuite.v1.MakerSuiteService/UpdatePrompt"
        },
        NETWORK: {
            RPC_ORIGIN: "https://alkalimakersuite-pa.clients6.google.com"
        },
        EVENTS: {
            REQUEST: "aistudio-compressor:generatecontent-request",
            RESPONSE: "aistudio-compressor:generatecontent-response"
        },
        UI: {
            BUTTON_ID: "compressor-button-hybrid",
            TOOLBAR_SELECTORS: [
                "ms-toolbar .toolbar-right",
                "ms-playground-toolbar .toolbar-right",
                "ms-playground-toolbar .toolbar-container .toolbar-right",
                ".toolbar-right"
            ],
            TARGET_PATH: "/prompts/",
            INPUT_SELECTORS: [
                "ms-autosize-textarea textarea",
                'textarea[aria-label="Start typing a prompt"]',
                "textarea"
            ],
            SEND_BUTTON_SELECTORS: [
                "ms-run-button button",
                "ms-run-button button.run-button",
                'button[aria-label="Run"]',
                'button[aria-label^="Run"]',
                'button[description*="Send prompt"]',
                'button[aria-label*="keyboard_return"]',
                'button[type="submit"]'
            ]
        },
        PROMPTS: {
            LEGACY_FULL: "Summarize the earlier role-play conversation as a compact, accurate memory snapshot.\nOnly summarize the history BEFORE the most recent completed exchanges identified in the final instruction. Preserve established characters, relationships, identity, world rules, important events, state changes, promises, possessions, unresolved conflicts, and the current situation before those exchanges. Avoid adding fictional details or overriding established facts.\nReturn only:\n<state_snapshot>\n[Concise, structured summary in the language of the conversation]\n</state_snapshot>",
            PREVIOUS_FULL: "请为这段长期对话制作按剧情时间线整理的精炼记忆，用于之后继续对话。仅总结后附说明指定的较早轮次。\n\n按以下顺序组织：\n【稳定设定】人物身份、关系、世界规则、持续有效的约定。\n【已发生的关键事件】按先后顺序记录重要行动、变化和结果；已结束的事件使用完成时态，标明结局和后续影响。\n【截至压缩截点的状态】只描述待删除历史结束时已经确立的状态、尚未解决的矛盾和后续伏笔，明确这是“压缩截点”而非整段对话的现在。\n【时间线衔接】说明最近保留轮次将接在什么事件之后。真正正在发生的剧情、地点和时间，应由保留的最近轮次决定。\n\n优先保留后续会影响人物决策和剧情因果的细节；输出应清晰、紧凑、忠于原文。仅返回上述结构化记忆，使用原对话语言。",
            FULL: "请只根据本次请求中实际提供的历史记录，制作供后续剧情使用的准确记忆摘要。按照事件发生顺序记录已完成的行动及后果、持续有效的人物关系和世界规则，以及记录末尾仍未解决的矛盾和伏笔。明确区分“已经结束的事件”和“在这些记录的末尾仍持续的状态”。所有状态都只截至本次提供的最后一条历史记录，不要推测未提供的后续剧情，也不要把过去情节描述成此刻正在发生。保留时间、地点、因果和重要关系的变化，忠实于原文，使用原对话语言，尽可能精炼。",
            SNIPPET: "请为这段长期对话制作按剧情时间线整理的精炼记忆"
        }
    };

    const env = {
        isMac: /Mac|iPhone|iPad/.test(navigator.platform),
        isAppContext: location.pathname.startsWith("/app/"),
        topWindow: unsafeWindow?.top || window
    };
    const SCRIPT_VERSION = "0.3.2";

    const state = {
        compressionInProgress: false,
        compressionRequestPending: false,
        compressionResponsePending: false,
        lastPromptHistory: null,
        lastPromptHistoryUpdatedAt: 0,
        lastResolvedPromptThread: null,
        lastResolvedPromptRoot: null,
        lastResolvedPromptId: null,
        lastResolvedPromptUpdatedAt: 0,
        googleApiKey: "",
        googleAuthUser: "0",
        googleApiKeyUpdatedAt: 0,
        historyCapturePending: false,
        branchInProgress: false,
        activeCompressionPrompt: "",
        activeCompressionMarker: "",
        activeCompressionKeepRounds: 0,
        scopedCompression: null,
        activeCompressionSnippet: Config.PROMPTS.SNIPPET
    };

    const log = {
        ui: (...args) => console.log(Config.TAGS.UI, ...args),
        net: (...args) => console.log(Config.TAGS.NET, ...args)
    };

    const STORAGE_KEY = "__aistudio_compressor_settings_v1";
    const DEFAULT_SETTINGS = {
        compressPrompt: Config.PROMPTS.FULL,
        snapshotRegex: "",
        keepRounds: 8,
        autoInject: false
    };
    let userSettings = loadUserSettings();
    refreshActivePromptFromSettings();

    function loadUserSettings() {
        try {
            const raw = localStorage.getItem(STORAGE_KEY);
            if (!raw) {
                return { ...DEFAULT_SETTINGS };
            }
            const parsed = JSON.parse(raw);
            if (parsed && typeof parsed === "object") {
                // Existing installations store the original default as a
                // literal string. Upgrade only that exact default and leave
                // user-edited summary prompts untouched.
                if ([Config.PROMPTS.LEGACY_FULL, Config.PROMPTS.PREVIOUS_FULL]
                    .includes(parsed.compressPrompt)) {
                    parsed.compressPrompt = Config.PROMPTS.FULL;
                    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(parsed)); }
                    catch {}
                }
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
        confirmScoped(promptId, marker, stats) {
            const previous = this.read(promptId);
            if (!previous || previous.marker !== marker ||
                stats?.recentRounds !== previous.keepRounds ||
                !Number.isSafeInteger(stats.olderRounds) || stats.olderRounds < 1) {
                throw new Error("摘要任务与已裁剪的模型请求不一致");
            }
            this.save({ ...previous, scopeVerified: true,
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
                if (remainingFields.includes(text)) {
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

    const NetworkMonitor = {
        init() {
            const label = env.isAppContext ? "app-window" : "top-window";
            hookContext(window, label);

            window.addEventListener("DOMContentLoaded", () => {
                hookContext(window, `${label} DOMContentLoaded`);
                hookChildFrames("DOMContentLoaded");
            });

            window.addEventListener("load", () => {
                hookContext(window, `${label} load`);
                hookChildFrames("load");
            });

            setInterval(() => hookChildFrames("poll"), 2000);
        }
    };

    function hookChildFrames(reason) {
        if (!document?.querySelectorAll) return;
        for (const frame of document.querySelectorAll("iframe")) {
            const ctx = frame.contentWindow;
            if (!ctx || !canAccessWindow(ctx)) continue;
            try {
                hookContext(ctx, `iframe ${frame.src || frame.name || "unnamed"} via ${reason}`);
            } catch (err) {
                log.net("cannot hook iframe:", err);
            }
        }
    }

    function hookContext(ctx, label) {
        if (!canAccessWindow(ctx)) {
            log.net(`skip hooking for ${label}: cross-origin window`);
            return;
        }
        installFetchHook(ctx, label);
        installXHRHook(ctx, label);
    }

    function canAccessWindow(ctx) {
        try {
            return ctx && ctx.location && ctx.location.origin === location.origin;
        } catch {
            return false;
        }
    }

    function installFetchHook(ctx, label) {
        if (!ctx || typeof ctx.fetch !== "function") {
            log.net(`skip hooking for ${label}: fetch not available`);
            return;
        }
        if (ctx.__aistudio_compressor_fetch_hooked) return;

        const originalFetch = ctx.fetch.bind(ctx);

        ctx.fetch = async function interceptFetch(input, init = {}) {
            const url = typeof input === "string" ? input : input?.url;
            const shouldLogGenerate = isTargetRequest(url, "GENERATE");
            const shouldCaptureResolve = isTargetRequest(url, "RESOLVE");
            const shouldMonitorSave = isTargetRequest(url, "UPDATE");
            if (shouldMonitorSave) {
                const result = await originalFetch(input, init);
                SaveMonitor.record(result.status, url);
                return result;
            }
            if (!shouldLogGenerate && !shouldCaptureResolve) {
                return originalFetch(input, init);
            }
            let request;
            try {
                request = new Request(input, init);
            } catch {
                // Never break the host application because our inspector cannot clone a request.
                return originalFetch(input, init);
            }
            let actualRequest = request;
            if (shouldLogGenerate && state.compressionInProgress && state.activeCompressionMarker) {
                try {
                    const scope = RequestScope.prepare(await request.clone().text(),
                        state.activeCompressionMarker, state.activeCompressionKeepRounds);
                    if (!scope) throw new Error("本次请求未包含有效的摘要任务标识");
                    actualRequest = new Request(request, { body: scope.bodyText });
                    state.scopedCompression = scope;
                    CompactionTask.confirmScoped(Branching.getCurrentPromptId(),
                        state.activeCompressionMarker, scope);
                    PromptPanel.status("模型请求已仅保留较早 " + scope.olderRounds +
                        " 轮历史；最近 " + scope.recentRounds + " 轮未发送");
                } catch (error) {
                    PromptPanel.status("摘要请求已阻止：" + String(error?.message || error));
                    Compression.finalizeRun();
                    throw error;
                }
            }
            if (shouldLogGenerate) await logRequestPayload(actualRequest);
            const response = await originalFetch(
                shouldLogGenerate && actualRequest !== request ? actualRequest : input,
                shouldLogGenerate && actualRequest !== request ? undefined : init);

            if (shouldLogGenerate || shouldCaptureResolve) {
                void logResponsePayload(request.url, response.clone());
            }

            return response;
        };

        ctx.__aistudio_compressor_fetch_hooked = true;
        log.net(`fetch hooked in ${label}`);

        if (ctx !== unsafeWindow && typeof unsafeWindow.fetch === "function" && !unsafeWindow.__aistudio_compressor_fetch_hooked) {
            unsafeWindow.fetch = ctx.fetch;
            unsafeWindow.__aistudio_compressor_fetch_hooked = true;
            log.net("fetch mirrored to unsafeWindow");
        }
    }

    async function logRequestPayload(request) {
        try {
            const clonedRequest = request.clone();
            const bodyText = await clonedRequest.text();
            const headers = Object.fromEntries(clonedRequest.headers.entries());
            rememberGoogleApiKey(headers, clonedRequest.url);

            // Sensitive headers are never logged.
            try {

            } catch {

            }

            emitNetworkEvent(Config.EVENTS.REQUEST, {
                url: clonedRequest.url,
                headers,
                bodyText
            });
        } catch (err) {
            console.warn(Config.TAGS.NET, "Failed to read request body:", err);
        }
    }

    async function logResponsePayload(url, response) {
        try {
            const bodyText = await response.text();
            const parsed = safeParseJSON(bodyText);
            maybeCaptureResolvedPromptThread(url, bodyText, parsed);


            if (isTargetRequest(url, "RESOLVE")) {

            } else if (parsed) {

            } else {

            }

            emitNetworkEvent(Config.EVENTS.RESPONSE, {
                url,
                status: response.status,
                statusText: response.statusText,
                bodyText
            });
        } catch (err) {
            console.warn(Config.TAGS.NET, "Failed to read response body:", err);
        }
    }

    function installXHRHook(ctx, label) {
        if (!ctx || typeof ctx.XMLHttpRequest !== "function") {
            log.net(`skip hooking XHR for ${label}: XMLHttpRequest not available`);
            return;
        }
        if (ctx.__aistudio_compressor_xhr_hooked) return;

        const OriginalXHR = ctx.XMLHttpRequest;

        function WrappedXHR() {
            const realXHR = new OriginalXHR();
            let monitored = false;
            let requestUrl = null;
            const requestHeaders = {};

            const originalOpen = realXHR.open;
            realXHR.open = function (...args) {
                const [, url] = args;
                requestUrl = url || null;
                monitored = Boolean(url) && (isTargetRequest(url, "GENERATE") ||
                    isTargetRequest(url, "RESOLVE") || isTargetRequest(url, "UPDATE"));
                if (monitored) {
                    log.net("Monitored XHR detected in", label, url);
                }
                return originalOpen.apply(this, args);
            };

            const originalSetRequestHeader = realXHR.setRequestHeader;
            realXHR.setRequestHeader = function (name, value) {
                if (typeof name === "string") {
                    requestHeaders[name] = value;
                    rememberGoogleApiKey(requestHeaders, resolveUrl(requestUrl));
                }
                return originalSetRequestHeader.apply(this, arguments);
            };

            const originalSend = realXHR.send;
            realXHR.send = function (body) {
                if (monitored) {
                    const absoluteUrl = resolveUrl(requestUrl);
                    let bodyText = bodyToText(body);
                    if (isTargetRequest(absoluteUrl, "GENERATE") &&
                        state.compressionInProgress && state.activeCompressionMarker) {
                        try {
                            const scope = RequestScope.prepare(bodyText,
                                state.activeCompressionMarker, state.activeCompressionKeepRounds);
                            if (!scope) throw new Error("本次请求缺少有效的摘要任务标识");
                            bodyText = scope.bodyText;
                            body = scope.bodyText;
                            state.scopedCompression = scope;
                            CompactionTask.confirmScoped(Branching.getCurrentPromptId(),
                                state.activeCompressionMarker, scope);
                            PromptPanel.status("模型请求已仅保留旧 " + scope.olderRounds +
                                " 轮；最近 " + scope.recentRounds + " 轮未发送");
                        } catch (error) {
                            PromptPanel.status("摘要请求已阻止：" + String(error?.message || error));
                            Compression.finalizeRun();
                            throw error;
                        }
                    }
                    const headers = Object.keys(requestHeaders).length ? { ...requestHeaders } : null;
                    rememberGoogleApiKey(headers, absoluteUrl);


                    if (bodyText) {
                        try {

                        } catch {

                        }
                    } else {

                    }

                    emitNetworkEvent(Config.EVENTS.REQUEST, {
                        url: absoluteUrl,
                        headers,
                        bodyText
                    });
                }
                return originalSend.apply(this, arguments);
            };

            realXHR.addEventListener("readystatechange", function () {
                if (realXHR.readyState !== 4 || !monitored) {
                    return;
                }
                const responseUrl = realXHR.responseURL || resolveUrl(requestUrl);
                if (isTargetRequest(responseUrl, "UPDATE")) {
                    SaveMonitor.record(realXHR.status, responseUrl);
                    return;
                }
                const parsed = safeParseJSON(realXHR.responseText);
                maybeCaptureResolvedPromptThread(responseUrl, realXHR.responseText, parsed);


                if (isTargetRequest(responseUrl, "RESOLVE")) {

                } else if (parsed) {

                } else {

                }

                emitNetworkEvent(Config.EVENTS.RESPONSE, {
                    url: responseUrl,
                    status: realXHR.status,
                    statusText: realXHR.statusText,
                    bodyText: realXHR.responseText
                });
            });

            return realXHR;
        }

        ctx.XMLHttpRequest = WrappedXHR;
        ctx.__aistudio_compressor_xhr_hooked = true;
        log.net(`XMLHttpRequest hooked in ${label}`);

        if (ctx !== unsafeWindow && typeof unsafeWindow.XMLHttpRequest === "function" && !unsafeWindow.__aistudio_compressor_xhr_hooked) {
            unsafeWindow.XMLHttpRequest = WrappedXHR;
            unsafeWindow.__aistudio_compressor_xhr_hooked = true;
            log.net("XMLHttpRequest mirrored to unsafeWindow");
        }
    }

    function resolveUrl(url) {
        if (!url) return "";
        try {
            return new URL(url, location.origin).toString();
        } catch {
            return url;
        }
    }

    function bodyToText(body) {
        if (!body) return null;
        if (typeof body === "string") return body;
        if (body instanceof ArrayBuffer) {
            try {
                return new TextDecoder().decode(body);
            } catch {
                return null;
            }
        }
        if (ArrayBuffer.isView(body)) {
            try {
                return new TextDecoder().decode(body.buffer);
            } catch {
                return null;
            }
        }
        return null;
    }

    function isTargetRequest(url, key = "GENERATE") {
        try {
            const parsed = new URL(url, location.origin);
            return parsed.pathname.endsWith(Config.TARGETS[key]);
        } catch {
            return false;
        }
    }

    // ---------------------------
    // Compression lifecycle
    // ---------------------------

    const Compression = {
        run() {
            if (state.compressionInProgress || state.branchInProgress) {
                log.ui("Compression or deletion already running");
                return;
            }
            if (InPlaceCompaction.pending) {
                PromptPanel.status("请先确认或取消已经生成的摘要");
                return;
            }
            const textarea = UI.findInput();
            if (!textarea) {
                log.ui("Input box not found, cannot run compression");
                return;
            }
            const promptId = Branching.getCurrentPromptId();
            if (!promptId || promptId === "new_chat") {
                PromptPanel.status("先保存当前聊天，再执行摘要压缩");
                return;
            }
            const task = CompactionTask.create(promptId, Number(userSettings.keepRounds) || 8);
            const prompt = CompactionTask.compose(getCompressPrompt(), task);
            CompactionTask.save(task);
            Compression.prepareRunContext();
            state.activeCompressionPrompt = prompt;
            state.activeCompressionSnippet = deriveSnippet(getCompressPrompt());
            state.activeCompressionMarker = task.marker;
            state.activeCompressionKeepRounds = task.keepRounds;
            state.scopedCompression = null;
            state.compressionInProgress = true;
            state.compressionRequestPending = true;
            state.compressionResponsePending = false;
            UI.setButtonLoading(true);
            setTextareaValue(textarea, prompt);
            textarea.focus();
            textarea.dispatchEvent(new InputEvent("input", { bubbles: true, data: prompt, inputType: "insertText" }));
            textarea.dispatchEvent(new Event("change", { bubbles: true }));
            sendCompressionRequest().then(sent => {
                if (sent) {
                    log.ui("Compression click sent; listening without a time limit");
                    return;
                }
                log.ui("Compression request was not confirmed; no retry attempted");
                PromptPanel.status("未确认摘要请求。为避免误触停止按钮，插件已停止监听；请检查当前聊天是否仍在生成。");
                Compression.finalizeRun();
            }).catch(error => {
                log.ui("Compression single click failed", error);
                PromptPanel.status("摘要发送失败：" + String(error?.message || error));
                Compression.finalizeRun();
            });
        },

        prepareRunContext() {
            state.lastPromptHistory = null;
            state.lastPromptHistoryUpdatedAt = 0;
            state.historyCapturePending = true;
            state.compressionRequestPending = false;
            state.compressionResponsePending = false;
        },

        handleRequestEvent(event) {
            const detail = event?.detail;
            if (!isTargetRequest(detail?.url, "GENERATE") || !detail?.bodyText) return;
            const isCompressionBody = Compression.isCompressionPrompt(detail.bodyText);

            if (!state.historyCapturePending && !state.compressionInProgress) {
                log.net("GenerateContent request observed outside compression run");
                return;
            }

            if (isCompressionBody && state.historyCapturePending) {
                state.historyCapturePending = false;
                const payload = safeParseJSON(detail.bodyText);
                if (payload) {
                    state.lastPromptHistory = payload;
                    state.lastPromptHistoryUpdatedAt = Date.now();
                    log.net("Stored prompt history snapshot for compression", {
                        ...describePromptHistory(payload)
                    });
                } else {
                    log.net("Compression request detected but prompt history parse failed");
                }
            } else if (state.historyCapturePending && !isCompressionBody) {
                log.net("Compression history capture pending, skipping non-compression request");
            }

            if (isCompressionBody && state.compressionInProgress) {
                state.compressionRequestPending = false;
                state.compressionResponsePending = true;
                log.net("Compression GenerateContent request captured");
            } else {
                log.net("Regular GenerateContent request observed");
            }
        },

        handleResponseEvent(event) {
            if (!state.compressionInProgress || !state.compressionResponsePending) {
                log.net("GenerateContent response event ignored", {
                    compressionInProgress: state.compressionInProgress,
                    compressionResponsePending: state.compressionResponsePending
                });
                return;
            }
            const detail = event?.detail;
            if (!isTargetRequest(detail?.url, "GENERATE")) return;
            if (!detail?.bodyText) {
                log.net("GenerateContent response missing bodyText");
                Compression.finalizeRun();
                return;
            }
            const data = safeParseJSON(detail.bodyText);
            if (!data) {
                log.net("Failed to parse compression response");
                Compression.finalizeRun();
                return;
            }
            const snapshot = extractStateSnapshot(data);
            if (!snapshot) {
                log.net("No snapshot found in compression response");
                Compression.finalizeRun();
                return;
            }
            log.net("Summary response completed; ready for manual review", { length: snapshot.length });
            PromptPanel.status("AI Studio 已返回摘要。请先确认模型已经完成，再点击「读取已完成摘要」核对并准备清理。");
            Compression.finalizeRun();
        },

        stopTracking() {
            if (!state.compressionInProgress) {
                PromptPanel.status("当前没有正在监听的摘要请求");
                return;
            }
            Compression.finalizeRun();
            PromptPanel.status("已停止插件监听，AI Studio 正在生成的内容不会被中断；完成后可以手动读取摘要。");
        },

        async recover() {
            if (state.branchInProgress) {
                PromptPanel.status("正在清理旧消息，请先完成当前操作");
                return;
            }
            try {
                PromptPanel.status("正在从当前会话重新读取已保存的摘要……");
                await recoverLatestCompletedSummary();
                Compression.finalizeRun();
            } catch (error) {
                PromptPanel.status("尚未取得可用于清理的完整摘要：" + String(error?.message || error));
                log.ui("Manual summary recovery failed", String(error?.message || error));
            }
        },

        finalizeRun() {
            state.compressionRequestPending = false;
            state.compressionResponsePending = false;
            state.compressionInProgress = false;
            state.historyCapturePending = false;
            UI.setButtonLoading(false);
        },

        isCompressionPrompt(bodyText) {
            if (typeof bodyText !== "string") return false;
            if (state.activeCompressionMarker && bodyText.includes(state.activeCompressionMarker)) return true;
            const snippet = state.activeCompressionSnippet || deriveSnippet(getCompressPrompt());
            if (!snippet) {
                return bodyText.includes(Config.PROMPTS.SNIPPET);
            }
            const variants = buildSnippetVariants(snippet);
            for (const candidate of variants) {
                if (candidate && bodyText.includes(candidate)) {
                    return true;
                }
            }
            return false;
        }
    };

    function isStopControl(button) {
        const icon = button.querySelector?.("mat-icon, .material-symbols-outlined");
        const label = [
            button.getAttribute?.("aria-label"),
            button.getAttribute?.("title"),
            button.getAttribute?.("iconname"),
            icon?.textContent,
            button.textContent
        ].filter(Boolean).join(" ").toLowerCase();
        return /(^|[^a-z])(stop|cancel|interrupt|abort)([^a-z]|$)|停止|中断生成|取消生成/.test(label);
    }

    async function sendCompressionRequest() {
        // Wait for the site's reactive input state to update. Never retry
        // a click: the original Run control may turn into Stop.
        await new Promise(resolve => requestAnimationFrame(resolve));
        if (!state.compressionInProgress) return false;
        const button = UI.findSendButton();
        if (!button || button.disabled || button.getAttribute?.("aria-disabled") === "true" || isStopControl(button)) {
            PromptPanel.status("未发送：原生运行按钮尚未就绪或正在生成。请检查输入框里的摘要指令后手动点击一次运行。");
            log.ui("Compression not auto-sent: Run button not ready");
            return false;
        }
        // One native click. No synthetic pointer sequence, force-enabling
        // the button, keyboard fallback, or second click on missed requests.
        button.click();
        log.ui("Compression request: single native Run click");
        PromptPanel.status("已点击运行一次，等待 AI Studio 接收请求；不会自动再次点击");
        // Keep the capture handlers active for as long as the model needs.
        // Recovery is a separate user-initiated step, so no timeout or retry.
        return true;
    }

    function setTextareaValue(textarea, value) {
        if ("value" in textarea) {
            const descriptor = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value");
            const setter = descriptor?.set;
            if (setter) {
                setter.call(textarea, value);
                return;
            }
            textarea.value = value;
        } else {
            textarea.textContent = value;
        }
    }

    // ---------------------------
    // History processing
    // ---------------------------

const RoundSplitter = Object.freeze({
    // Pinned compaction memories can remain as standalone model messages
    // after their temporary tool prompts are removed. They are never counted
    // as conversational user/model rounds.
    isMemory(turn) {
      if (turn?.role !== "model") return false;
      const content = typeof turn.text === "string" ? turn.text :
        JSON.stringify(turn.entry?.[0] ?? "");
      return /\[\[SUJIU_MEMORY_V1:[a-z0-9-]{8,80}\]\]/i.test(content);
    },
    partition(turns, keepRounds = 8) {
      if (!Array.isArray(turns) || !Number.isSafeInteger(keepRounds) || keepRounds < 1) {
        throw new Error("Invalid conversation history or keepRounds");
      }
      const units = [];
      let current = null;
      function closeRound() {
        if (!current) return;
        if (!current.some(turn => turn.role === "model")) {
          throw new Error("Conversation contains an incomplete response");
        }
        units.push({ kind: "round", turns: current });
        current = null;
      }
      for (const turn of turns) {
        if (!turn || typeof turn.role !== "string") {
          throw new Error("Conversation contains an invalid turn");
        }
        if (turn.role === "user") {
          closeRound();
          current = [turn];
        } else if (this.isMemory(turn) && (!current ||
          (current.some(item => item.role === "model") &&
           !/\[\[SUJIU_SCOPE_V2:/i.test(current[0]?.text ||
               JSON.stringify(current[0]?.entry?.[0] ?? ""))))) {
          closeRound();
          units.push({ kind: "memory", turns: [turn] });
        } else if (!current) {
          throw new Error("Conversation starts with a non-user turn");
        } else {
          current.push(turn);
        }
      }
      closeRound();
      const regularRounds = units.filter(unit => unit.kind === "round").length;
      if (regularRounds <= keepRounds) {
        throw new Error("Not enough completed rounds to compress");
      }
      let seen = 0;
      let keepIndex = -1;
      for (let i = units.length - 1; i >= 0; i--) {
        if (units[i].kind === "round" && ++seen === keepRounds) {
          keepIndex = i;
          break;
        }
      }
      if (keepIndex < 0) throw new Error("Cannot identify recent turns");
      const older = units.slice(0, keepIndex).flatMap(unit => unit.turns);
      const recent = units.slice(keepIndex).flatMap(unit => unit.turns);
      return {
        older, recent,
        olderRounds: regularRounds - keepRounds,
        recentRounds: keepRounds,
        totalRounds: regularRounds,
        pinnedMemories: units.filter(unit => unit.kind === "memory").length
      };
    }
  });

    function findSavedSummary(turns, promptId) {
        // A thinking model may persist additional model/thought entries after
        // its visible reply. Identify the last USER turn, rather than assuming
        // the last two serialized records are exactly user/model.
        const lastUserIndex = turns.findLastIndex(turn => turn?.role === "user");
        if (lastUserIndex < 0) throw new Error("会话中没有可识别的用户消息");
        const latestUser = turns[lastUserIndex];
        const task = CompactionTask.parsePrompt(latestUser?.text || "");
        if (!task) {
            const trailingRoles = turns.slice(-5).map(turn => turn.role).join(", ");
            log.ui("Summary marker not found on last user turn", {
                trailingRoles,
                lastUserIndex,
                totalEntries: turns.length,
                lastUserLength: latestUser?.text?.length || 0
            });
            throw new Error("最新用户消息没有素酒摘要任务标识（旧版兼容标识也未匹配）。请确认 Google 已保存摘要，并且摘要之后没有发送其他用户消息");
        }
        const stored = CompactionTask.read(promptId);
        if (!stored || !stored.scopeVerified || task.marker !== stored.marker) {
            throw new Error("无法确认本次模型请求已经裁掉近期对话；拒绝打开清理功能");
        }
        const responses = turns.slice(lastUserIndex + 1);
        if (!responses.length || responses.some(turn => turn.role !== "model")) {
            throw new Error("摘要后尚无完整的模型回复，或会话结构已变更");
        }
        const snapshot = responses
            .filter(turn => !isThoughtOnlyTurn(turn))
            .map(turn => String(turn.text || "").trim())
            .filter(Boolean).join("\n").trim();
        if (!snapshot) throw new Error("Google 端仍未保存可见的模型摘要，请等生成结束后重试");
        const pinnedMarker = CompactionTask.memoryMarker(task.marker);
        if (!snapshot.includes(pinnedMarker)) {
            throw new Error("摘要缺少永久记忆标识，无法确保后续压缩保护它；请重新生成摘要");
        }
        return {
            snapshot,
            pinnedMarker,
            lastUserIndex,
            marker: task.marker,
            keepRounds: stored.keepRounds,
            originalRounds: stored.totalRounds,
            legacy: false
        };
    }

    async function recoverLatestCompletedSummary() {
        const id = Branching.getCurrentPromptId();
        if (!id || id === "new_chat") {
            throw new Error("当前聊天还没有保存为独立会话");
        }
        const button = UI.findSendButton();
        if (button && isStopControl(button)) {
            throw new Error("AI Studio 仍在生成中，请等停止按钮恢复为运行按钮后重试");
        }
        // Read the live server copy, never an older intercepted response.
        const capture = await fetchResolvedPromptThread(id);
        if (!capture?.entries?.length) {
            throw new Error("未能读取 Google 保存的聊天记录，请确认网络与账号状态");
        }
        const turns = extractResolvedTurns(capture.entries);
        assertSupportedEntries(capture.entries, turns);
        if (turns.length < 4) throw new Error("尚无足够的完整聊天轮次");
        const task = findSavedSummary(turns, id);
        const originalTurns = turns.slice(0, task.lastUserIndex);
        const split = RoundSplitter.partition(originalTurns, task.keepRounds);
        if (split.totalRounds !== task.originalRounds) {
            throw new Error("Google 保存的原聊天轮数与压缩前不一致，可能是请求裁剪影响了保存；已停止删除");
        }
        InPlaceCompaction.stage(task.snapshot, split, task);
        log.ui("Saved summary recognized; awaiting explicit confirmation", {
            originalRounds: split.totalRounds,
            olderRounds: split.olderRounds,
            recentRounds: split.recentRounds,
            markerPresent: Boolean(task.marker),
            scopeVerified: true
        });
    }

    function extractStateSnapshot(events) {
        const fullText = collectModelOutputText(events);
        if (!fullText) {
            log.net("collectModelOutputText returned empty string");
            return null;
        }
        const pattern = getSnapshotRegex();
        if (pattern) {
            const matched = matchByRegex(fullText, pattern);
            if (matched) {
                log.net("state_snapshot extracted via custom regex");
                return matched;
            }
            log.net("Custom snapshot regex did not match content");
        }
        log.net("No custom regex provided, returning raw model output as snapshot");
        return fullText.trim();
    }

    function collectModelOutputText(events) {
        const eventList = normalizeEvents(events);
        if (!eventList.length) {
            return "";
        }
        let buffer = "";
        for (const event of eventList) {
            const candidates = Array.isArray(event?.[0]) ? event[0] : null;
            if (!candidates) continue;
            for (const candidate of candidates) {
                if (!Array.isArray(candidate)) continue;
                const contentBlock = Array.isArray(candidate[0]) ? candidate[0] : null;
                if (!contentBlock) continue;
                const parts = Array.isArray(contentBlock[0]) ? contentBlock[0] : null;
                if (!parts) continue;
                for (const part of parts) {
                    if (!Array.isArray(part)) continue;
                    const text = part[1];
                    const isThought = Boolean(part[12]);
                    if (typeof text === "string" && !isThought) {
                        buffer += text;
                    }
                }
            }
        }
        log.net("collectModelOutputText aggregated", {
            eventCount: eventList.length,
            textLength: buffer.length
        });
        return buffer;
    }

    function normalizeEvents(payload) {
        if (!Array.isArray(payload)) {
            return [];
        }
        if (payload.length === 1 && Array.isArray(payload[0])) {
            const inner = payload[0];
            if (inner.every(item => Array.isArray(item))) {
                return inner;
            }
        }
        return payload;
    }

    function extractTurns(payload) {
        try {
            if (!Array.isArray(payload)) {
                log.net("Prompt history payload is not an array");
                return [];
            }
            const rawTurns = payload[1];
            if (!Array.isArray(rawTurns)) {
                log.net("Prompt history array missing turn entries");
                return [];
            }
            const turns = [];
            for (const entry of rawTurns) {
                if (!Array.isArray(entry) || entry.length < 2) continue;
                const role = entry[1];
                const text = extractTurnText(entry[0], role);
                if (typeof role === "string" && text) {
                    turns.push({
                        role,
                        text,
                        entry: deepClone(entry)
                    });
                }
            }
            log.net("Extracted", turns.length, "turns from prompt history");
            return turns;
        } catch (err) {
            log.net("Failed to extract turns", err);
            return [];
        }
    }

    function assertSupportedEntries(entries, turns) {
        const messages = entries.filter(entry => Array.isArray(entry) && typeof entry[8] === "string");
        if (messages.length !== turns.length ||
            turns.some(turn => turn.role !== "user" && turn.role !== "model")) {
            throw new Error("此会话包含尚未支持的工具、多模态或空内容消息，已中止压缩以避免丢失内容");
        }
    }

    async function getRebuildSourceTurns() {
        const currentPromptId = Branching.getCurrentPromptId();
        if (state.lastResolvedPromptThread?.length) {
            if (!currentPromptId || !state.lastResolvedPromptId || state.lastResolvedPromptId === currentPromptId) {
                const resolvedTurns = extractResolvedTurns(state.lastResolvedPromptThread);
                assertSupportedEntries(state.lastResolvedPromptThread, resolvedTurns);
                if (resolvedTurns.length) {
                    log.net("Rebuilding conversation from resolved prompt thread", {
                        promptId: state.lastResolvedPromptId,
                        turnCount: resolvedTurns.length,
                        updatedAt: state.lastResolvedPromptUpdatedAt
                    });
                    return resolvedTurns;
                }
            } else {
                log.net("Resolved prompt thread does not match current prompt, skipping", {
                    currentPromptId,
                    resolvedPromptId: state.lastResolvedPromptId
                });
            }
        }

        const fetchedCapture = await ensureResolvedPromptThread(currentPromptId);
        if (fetchedCapture?.entries?.length) {
            const fetchedTurns = extractResolvedTurns(fetchedCapture.entries);
            assertSupportedEntries(fetchedCapture.entries, fetchedTurns);
            if (fetchedTurns.length) {
                log.net("Rebuilding conversation from on-demand resolved prompt thread", {
                    promptId: fetchedCapture.promptId,
                    turnCount: fetchedTurns.length
                });
                return fetchedTurns;
            }
        }

        if (!state.lastPromptHistory) {
            return [];
        }

        log.net("Rebuilding conversation using stored GenerateContent history", {
            summary: describePromptHistory(state.lastPromptHistory)
        });
        const fallback = extractTurns(state.lastPromptHistory);
        if (fallback.some(turn => turn.role !== "user" && turn.role !== "model")) {
            throw new Error("GenerateContent 历史包含未支持的消息角色");
        }
        return fallback;
    }

    async function ensureResolvedPromptThread(promptId) {
        if (!promptId) {
            return null;
        }
        if (state.lastResolvedPromptThread?.length && (!state.lastResolvedPromptId || state.lastResolvedPromptId === promptId)) {
            return {
                promptId: state.lastResolvedPromptId || promptId,
                title: getResolvedPromptTitle(state.lastResolvedPromptRoot),
                root: state.lastResolvedPromptRoot ? deepClone(state.lastResolvedPromptRoot) : null,
                entries: state.lastResolvedPromptThread.map(entry => deepClone(entry))
            };
        }
        return fetchResolvedPromptThread(promptId);
    }

    async function fetchResolvedPromptThread(promptId) {
        const authHeader = await buildSapisidAuthHeader();
        if (!authHeader) {
            log.net("Unable to build SAPISIDHASH auth header for ResolveDriveResource fetch");
            return null;
        }
        const endpoint = `${Config.NETWORK.RPC_ORIGIN}${Config.TARGETS.RESOLVE}`;
        const apiKey = getGoogleApiKey();
        try {
            const response = await fetch(endpoint, {
                method: "POST",
                credentials: "include",
                headers: {
                    "content-type": "application/json+protobuf",
                    "x-goog-api-key": apiKey,
                    "x-goog-authuser": state.googleAuthUser || "0",
                    authorization: authHeader
                },
                body: JSON.stringify([promptId])
            });
            const bodyText = await response.text();
            if (!response.ok) {
                log.net("ResolveDriveResource fetch failed", {
                    promptId,
                    status: response.status,
                    bodyText
                });
                return null;
            }
            const capture = extractResolvedPromptThread(safeParseJSON(bodyText));
            if (!capture?.entries?.length) {
                log.net("ResolveDriveResource fetch returned no reusable thread", { promptId });
                return null;
            }
            rememberResolvedPromptCapture(capture, "Fetched resolved prompt thread on demand");
            return capture;
        } catch (err) {
            log.net("ResolveDriveResource fetch threw", err);
            return null;
        }
    }

    function getCookieValue(name) {
        const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        const match = document.cookie.match(new RegExp(`(?:^|; )${escaped}=([^;]*)`));
        return match ? decodeURIComponent(match[1]) : "";
    }

    function getHeaderValue(headers, name) {
        if (!headers || typeof headers !== "object" || !name) {
            return "";
        }
        const normalizedName = String(name).toLowerCase();
        for (const [key, value] of Object.entries(headers)) {
            if (String(key).toLowerCase() === normalizedName && typeof value === "string") {
                return value;
            }
        }
        return "";
    }

    function rememberGoogleApiKey(headers, sourceUrl = "") {
        const candidate = getHeaderValue(headers, "x-goog-api-key").trim();
        if (!candidate) {
            return "";
        }
        if (candidate !== state.googleApiKey) {
            log.net("Captured Google API key from native request headers", {
                sourceUrl,
                length: candidate.length
            });
        }
        state.googleApiKey = candidate;
        const authUser = getHeaderValue(headers, "x-goog-authuser");
        if (/^\d+$/.test(authUser)) state.googleAuthUser = authUser;
        state.googleApiKeyUpdatedAt = Date.now();
        return candidate;
    }

    function getGoogleApiKey() {
        const apiKey = (state.googleApiKey || "").trim();
        if (!apiKey) {
            throw new Error("Google API key has not been captured yet; trigger one native AI Studio request first");
        }
        return apiKey;
    }

    async function buildSapisidAuthHeader() {
        const sapisid = getCookieValue("SAPISID") || getCookieValue("__Secure-3PAPISID") || getCookieValue("APISID");
        if (!sapisid || !globalThis.crypto?.subtle) {
            return null;
        }
        const timestamp = Math.floor(Date.now() / 1000);
        const origin = location.origin;
        const encoder = new TextEncoder();
        const source = `${timestamp} ${sapisid} ${origin}`;
        const digest = await globalThis.crypto.subtle.digest("SHA-1", encoder.encode(source));
        const hash = Array.from(new Uint8Array(digest))
            .map(byte => byte.toString(16).padStart(2, "0"))
            .join("");
        return `SAPISIDHASH ${timestamp}_${hash} SAPISID1PHASH ${timestamp}_${hash} SAPISID3PHASH ${timestamp}_${hash}`;
    }

    function extractResolvedTurns(entries) {
        if (!Array.isArray(entries)) {
            return [];
        }
        const turns = [];
        for (const entry of entries) {
            if (!Array.isArray(entry) || entry.length < 9) continue;
            const role = entry[8];
            const text = extractResolvedEntryText(entry);
            if (typeof role === "string" && text) {
                turns.push({
                    role,
                    text,
                    entry: deepClone(entry)
                });
            }
        }
        log.net("Extracted", turns.length, "turns from resolved prompt thread");
        return turns;
    }

    function extractResolvedEntryText(entry) {
        if (!Array.isArray(entry)) return "";
        if (typeof entry[0] === "string" && entry[0]) {
            return entry[0];
        }
        const parts = Array.isArray(entry[29]) ? entry[29] : null;
        if (!parts) {
            return "";
        }
        let buffer = "";
        for (const part of parts) {
            if (!Array.isArray(part)) continue;
            const chunk = part[1];
            if (typeof chunk === "string") {
                buffer += chunk;
            }
        }
        return buffer;
    }

    function rememberResolvedPromptCapture(capture, label = "Captured resolved prompt thread") {
        if (!capture?.entries?.length) {
            return;
        }
        state.lastResolvedPromptThread = capture.entries.map(entry => deepClone(entry));
        state.lastResolvedPromptRoot = Array.isArray(capture.root) ? deepClone(capture.root) : null;
        state.lastResolvedPromptId = capture.promptId;
        state.lastResolvedPromptUpdatedAt = Date.now();
        log.net(label, describeResolvedPromptCapture(capture));
    }

    function maybeCaptureResolvedPromptThread(url, bodyText, parsed = null) {
        if (!isTargetRequest(url, "RESOLVE") || typeof bodyText !== "string") {
            return;
        }
        const payload = parsed || safeParseJSON(bodyText);
        const capture = extractResolvedPromptThread(payload);
        if (!capture?.entries?.length) {
            return;
        }
        rememberResolvedPromptCapture(capture);
    }

    function collectPromptIds(value, matches = [], seen = new Set()) {
        if (value == null) {
            return matches;
        }
        if (typeof value === "string") {
            const match = value.match(/(?:^|\b)prompts\/([^/?#\s"']+)/);
            if (match && !matches.includes(match[1])) {
                matches.push(match[1]);
            }
            return matches;
        }
        if (typeof value !== "object") {
            return matches;
        }
        if (seen.has(value)) {
            return matches;
        }
        seen.add(value);
        if (Array.isArray(value)) {
            for (const item of value) {
                collectPromptIds(item, matches, seen);
            }
            return matches;
        }
        for (const nested of Object.values(value)) {
            collectPromptIds(nested, matches, seen);
        }
        return matches;
    }

    function extractResolvedPromptThread(payload) {
        if (!Array.isArray(payload) || !Array.isArray(payload[0])) {
            return null;
        }
        const root = payload[0];
        if (!Array.isArray(root) || root.length < 14) {
            return null;
        }
        const threads = Array.isArray(root[13]) ? root[13] : null;
        const primaryThread = Array.isArray(threads?.[0]) ? threads[0] : null;
        if (!primaryThread?.length) {
            return null;
        }
        return {
            promptId: extractMutationPromptId(root),
            title: getResolvedPromptTitle(root),
            root: deepClone(root),
            entries: primaryThread.filter(Array.isArray).map(entry => deepClone(entry))
        };
    }

    function describeResolvedPromptCapture(capture) {
        if (!capture) {
            return { valid: false };
        }
        return {
            valid: true,
            promptId: capture.promptId,
            title: capture.title || "",
            turnCount: capture.entries?.length || 0,
            firstRole: capture.entries?.[0]?.[8]
        };
    }


    function getResolvedPromptTitle(root) {
        const candidate = Array.isArray(root?.[4]) ? root[4][0] : null;
        if (typeof candidate === "string" && candidate.trim()) {
            return candidate.trim();
        }
        const heading = document.querySelector("main h1")?.textContent || document.querySelector("h1")?.textContent || "";
        return heading.trim();
    }

    function describePromptHistory(payload) {
        if (!Array.isArray(payload)) {
            return { valid: false, type: typeof payload };
        }
        const rawTurns = Array.isArray(payload[1]) ? payload[1] : null;
        if (!rawTurns) {
            return { valid: false, reason: "missing turns array" };
        }
        const firstTurn = rawTurns[0];
        return {
            valid: true,
            turnCount: rawTurns.length,
            firstRole: firstTurn?.[1],
            firstChunkType: Array.isArray(firstTurn?.[0]) ? "chunks" : typeof firstTurn?.[0]
        };
    }

    function stripCompressionArtifacts(turns) {
        if (!turns.length) return turns;
        const sanitized = turns.slice();
        let removed = 0;
        let removedSnapshotTurn = false;

        while (sanitized.length) {
            const lastTurn = sanitized[sanitized.length - 1];
            if (isCompressionPromptTurn(lastTurn)) {
                sanitized.pop();
                removed += 1;
                removedSnapshotTurn = false;
                continue;
            }
            if (isCompressionSnapshotTurn(lastTurn)) {
                sanitized.pop();
                removed += 1;
                removedSnapshotTurn = true;
                continue;
            }
            if (removedSnapshotTurn && isThoughtOnlyTurn(lastTurn)) {
                sanitized.pop();
                removed += 1;
                continue;
            }
            break;
        }

        if (removed > 0) {
            log.net("Removed compression artifact turns from tail", {
                removed,
                remaining: sanitized.length
            });
        }
        return sanitized;
    }

    function isCompressionPromptTurn(turn) {
        const snippet = state.activeCompressionSnippet || deriveSnippet(getCompressPrompt());
        return Boolean(
            turn?.role === "user" &&
            typeof turn.text === "string" &&
            snippet &&
            turn.text.includes(snippet)
        );
    }

    function isCompressionSnapshotTurn(turn) {
        if (turn?.role !== "model" || typeof turn.text !== "string" || !turn.text) {
            return false;
        }
        if (/<state_snapshot>[\s\S]*<\/state_snapshot>/i.test(turn.text)) {
            return true;
        }
        const pattern = getSnapshotRegex();
        if (pattern && matchByRegex(turn.text, pattern)) {
            return true;
        }
        return /<scratchpad>/i.test(turn.text) && /state[_ -]?snapshot/i.test(turn.text);
    }

    function isThoughtOnlyTurn(turn) {
        return Boolean(
            turn?.role === "model" &&
            Array.isArray(turn.entry) &&
            turn.entry.length >= 33 &&
            turn.entry[19] === 1
        );
    }

    function extractTurnText(rawContent, role) {
        if (!rawContent) return "";
        if (typeof rawContent === "string") return rawContent;
        if (!Array.isArray(rawContent)) return "";

        let buffer = "";
        const shouldSkipFirstBlock = role === "model";
        let skipThoughtChunk = shouldSkipFirstBlock;
        let skippedChunk = "";
        for (const block of rawContent) {
            if (!Array.isArray(block)) continue;
            const isThought = Boolean(block[12]);
            if (isThought) continue;
            const chunk = block[1];
            if (typeof chunk === "string") {
                if (skipThoughtChunk) {
                    skipThoughtChunk = false;
                    skippedChunk = chunk;
                    continue;
                }
                buffer += chunk;
            }
        }
        if (!buffer && skippedChunk) {
            return skippedChunk;
        }
        return buffer;
    }

    function createTurn(role, text) {
        return { role, text };
    }

    // ---------------------------
    // Branching controls
    // ---------------------------

    // Kept as a stable prompt locator. Compaction never creates a new prompt.
    const Branching = Object.freeze({
        getCurrentPromptId() {
            const match = (location.pathname || "").match(/\/prompts\/([^/?#]+)/);
            return match ? match[1] : null;
        }
    });


    const InPlaceCompaction = (() => {
        let pending = null;
        let deleting = false;
        const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

        function normalizeType(turn) {
            const container = turn.querySelector(".chat-turn-container");
            if (container?.classList?.contains("user")) return "user";
            if (turn.classList?.contains("thought-activity-host")) return "thought";
            if (turn.querySelector("ms-thought-chunk, .thought-panel") &&
                !turn.querySelector(".turn-content ms-text-chunk, .turn-content ms-code-block, .turn-content .cmark-node")) {
                return "thought";
            }
            const raw = (turn.getAttribute("data-turn-type") ||
                turn.getAttribute("data-turn-role") ||
                turn.dataset?.turnType || turn.dataset?.turnRole || "").toLowerCase();
            if (raw === "user" || raw === "prompt") return "user";
            if (raw === "thought") return "thought";
            if (raw === "model" || raw === "text" || raw === "response") return "model";
            if (container?.classList?.contains("model") ||
                container?.classList?.contains("render") ||
                turn.querySelector(".turn-content, .virtual-scroll-container")) return "model";
            return "unknown";
        }

        // Use every rendered native turn, including thought blocks. An
        // incomplete / virtualized DOM is never safe for destructive changes.
        function collectGroups(turns) {
            const groups = [];
            for (const turn of turns) {
                const type = normalizeType(turn);
                if (type === "unknown") throw new Error("发现未识别的消息类型，已停止删除");
                if (type === "user") {
                    groups.push({ turns: [turn], hasModel: false, kind: "round" });
                } else {
                    const group = groups[groups.length - 1];
                    const hasMemory = type === "model" &&
                        CompactionTask.isPinnedSummary(turn.textContent || "");
                    const toolPrompt = group?.turns?.[0]?.textContent?.includes("[[SUJIU_SCOPE_V2:");
                    if (hasMemory && (!group || (group.hasModel && !toolPrompt))) {
                        groups.push({ turns: [turn], hasModel: true, kind: "memory" });
                        continue;
                    }
                    if (!group) throw new Error("历史消息的顺序异常");
                    group.turns.push(turn);
                    if (type === "model") group.hasModel = true;
                }
            }
            if (groups.some(group => !group.hasModel)) throw new Error("发现未完成的轮次，已停止删除");
            return groups;
        }

        function stage(snapshot, split, task = null) {
            const id = Branching.getCurrentPromptId();
            if (!id || id === "new_chat") throw new Error("保存会话后才能原地压缩");
            if (!snapshot?.trim()) throw new Error("摘要为空，无法压缩");
            // Previously generated memories are pinned. Never select
            // their own model replies for deletion in a later compression.
            const pinnedOlderRounds = [];
            const pinnedMemoryTexts = [];
            let roundIndex = -1;
            for (const turn of split.older || []) {
                if (turn.role === "user") roundIndex++;
                if (turn.role === "model" && CompactionTask.isPinnedSummary(turn.text || "")) {
                    pinnedMemoryTexts.push(String(turn.text).trim());
                    if (roundIndex >= 0 && !pinnedOlderRounds.includes(roundIndex)) {
                        pinnedOlderRounds.push(roundIndex);
                    }
                }
            }
            pending = {
                pinnedOlderRounds,
                pinnedMemoryTexts,
                promptId: id,
                snapshot: snapshot.trim(),
                olderRounds: split.olderRounds,
                recentRounds: split.recentRounds,
                totalRounds: split.totalRounds,
                marker: task?.marker || null,
                memoryMarker: task?.pinnedMarker || (task?.marker ?
                    CompactionTask.memoryMarker(task.marker) : null),
                legacy: Boolean(task?.legacy),
                createdAt: Date.now()
            };
            PromptPanel.showCompactionReview(pending);
            return pending;
        }

        async function verifyPersistedSummary(plan) {
            const capture = await fetchResolvedPromptThread(plan.promptId);
            if (!capture?.entries?.length) throw new Error("未能读取最新会话，停止删除");
            assertSupportedEntries(capture.entries, extractResolvedTurns(capture.entries));
            const turns = extractResolvedTurns(capture.entries);
            const users = turns.filter(turn => turn.role === "user");
            if (users.length !== plan.totalRounds + 1) {
                throw new Error("会话轮数与生成摘要前不一致，请刷新后重新生成摘要");
            }
            const latest = findSavedSummary(turns, plan.promptId);
            if (latest.marker !== plan.marker ||
                latest.keepRounds !== plan.recentRounds ||
                latest.snapshot.trim() !== plan.snapshot.trim() ) {
                throw new Error("Google 端摘要或保留轮数与预览不一致，拒绝删除");
            }
            const previous = turns.slice(0, latest.lastUserIndex);
            const split = RoundSplitter.partition(previous, plan.recentRounds);
            if (split.totalRounds !== plan.totalRounds || split.olderRounds !== plan.olderRounds) {
                throw new Error("原聊天已经变化，请重新读取摘要再清理");
            }
            return { capture, turns };
        }

        function preflightDom(plan) {
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            const groups = collectGroups(turns);
            const regular = groups.filter(group => group.kind !== "memory");
            if (regular.length !== plan.totalRounds + 1) {
                throw new Error("网页未完整加载历史轮次（检测到 " +
                    regular.length + "，预期 " + (plan.totalRounds + 1) + "），拒绝删除");
            }
            const pinned = new Set(plan.pinnedOlderRounds || []);
            const getRole = turn => normalizeType(turn);
            for (const index of pinned) {
                if (index < 0 || index >= plan.olderRounds ||
                    !regular[index]?.turns?.some(turn =>
                        getRole(turn) === "model" &&
                        CompactionTask.isPinnedSummary(turn.textContent || ""))) {
                    throw new Error("历史压缩记忆与网页消息位置不一致，拒绝删除");
                }
            }

            // Regular old exchanges can be removed; completed summary rounds
            // are pinned in their entirety, so their visible memory survives.
            const targetTurns = [];
            for (let i = 0; i < plan.olderRounds; i++) {
                if (!pinned.has(i)) {
                    targetTurns.push(...[...regular[i].turns].reverse());
                }
            }

            // The newest summary is pinned by a separate marker on the MODEL
            // reply. The tool's temporary USER instruction and independent
            // thought cards are eligible for cleanup; never target its reply.
            const lastGroup = regular[regular.length - 1];
            let skippedThoughts = 0;
            if (plan.marker && plan.memoryMarker) {
                const user = lastGroup.turns[0];
                const memoryNodes = lastGroup.turns.filter(turn =>
                    getRole(turn) === "model" &&
                    (turn.textContent || "").includes(plan.memoryMarker));
                if (getRole(user) !== "user" ||
                    !(user.textContent || "").includes(plan.marker) ||
                    !memoryNodes.length) {
                    throw new Error("当前压缩摘要没有可见的永久标识，拒绝清理工具消息");
                }
                // Thought cards embedded inside a model response cannot be
                // removed independently without risking the summary itself.
                for (const turn of lastGroup.turns.slice(1)) {
                    if (getRole(turn) !== "thought") continue;
                    if (turn.querySelector("ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol")) {
                        targetTurns.push(turn);
                    } else {
                        skippedThoughts++;
                    }
                }
                targetTurns.push(user);
            }

            const targetIds = targetTurns.map(turn => turn.id);
            if (targetIds.some(id => !id) || new Set(targetIds).size !== targetIds.length) {
                throw new Error("待删除消息缺少稳定 ID，拒绝执行");
            }
            const targetSet = new Set(targetIds);
            const protectedTurns = turns.filter(turn => !targetSet.has(turn.id));
            if (protectedTurns.some(turn => !turn.id || targetSet.has(turn.id))) {
                throw new Error("保护内容与删除范围发生冲突");
            }
            if (targetTurns.some(turn => !turn.querySelector(
                "ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"
            ))) {
                throw new Error("待删除消息缺少独立的原生删除操作，拒绝执行");
            }
            const deletedUserCount = targetTurns.filter(turn => getRole(turn) === "user").length;
            const deletedOldRounds = plan.olderRounds - pinned.size;
            return {
                targetIds,
                protectedIds: protectedTurns.map(turn => turn.id),
                deletedUserCount, deletedOldRounds, pinnedCount: pinned.size,
                skippedThoughts, groups: groups.length, items: targetIds.length
            };
        }

        // Native message menus can autosave independently, but the plugin
        // performs only one final Google cloud read after the whole batch.
        async function awaitCloudPersistence(plan, expectedUsers, expectedProtected, preview, saveBaseline) {
            let lastUsers = null;
            for (let i = 0; i < 8; i++) {
                await sleep(i === 0 ? 800 : 1700);
                const capture = await fetchResolvedPromptThread(plan.promptId);
                if (!capture?.entries?.length) continue;
                const turns = extractResolvedTurns(capture.entries);
                assertSupportedEntries(capture.entries, turns);
                const users = turns.filter(turn => turn.role === "user");
                lastUsers = users.length;
                if (lastUsers < expectedUsers) {
                    throw new Error("云端消息少于计划保留的数量，可能误删了内容");
                }
                if (lastUsers !== expectedUsers) continue;

                // Exact current and previous pinned summaries must remain,
                // even when the synthetic tool prompt has been deleted.
                const memory = turns.find(turn => turn.role === "model" &&
                    (turn.text || "").includes(plan.memoryMarker || "UNMATCHABLE_MARKER"));
                if (plan.memoryMarker && (!memory ||
                    String(memory.text).trim() !== plan.snapshot.trim())) {
                    throw new Error("Google 云端的最新压缩记忆丢失或发生变化");
                }
                for (const pinnedText of plan.pinnedMemoryTexts || []) {
                    if (!turns.some(turn => turn.role === "model" &&
                        String(turn.text || "").trim() === pinnedText)) {
                        throw new Error("以前压缩过的记忆丢失，立即停止");
                    }
                }

                // Compare the last N original conversation rounds, excluding
                // the new synthetic summary model reply from the tail.
                const beforeMemory = plan.memoryMarker
                    ? turns.slice(0, turns.findIndex(turn => turn.role === "model" &&
                        (turn.text || "").includes(plan.memoryMarker)))
                    : turns.slice(0, -2);
                const userStarts = beforeMemory.flatMap((turn, index) =>
                    turn.role === "user" ? [index] : []);
                if (userStarts.length < plan.recentRounds) {
                    throw new Error("云端最近保留轮次不足");
                }
                const recent = beforeMemory.slice(userStarts[userStarts.length - plan.recentRounds]);
                if (JSON.stringify(recent.map(turn => [turn.role, turn.text])) !== expectedProtected) {
                    throw new Error("云端最近保留的正文与删除前不一致");
                }
                return true;
            }
            const save = SaveMonitor.attempts > saveBaseline
                ? "最近原生保存状态 " + (SaveMonitor.latest?.status || "未知")
                : "未观察到原生 UpdatePrompt";
            throw new Error("批量删除后 Google 未确认保存（云端用户消息 " +
                (lastUsers ?? "未知") + "，预期 " + expectedUsers + "；" + save + "）");
        }

        async function findNativeDeleteItem() {
            for (let attempt = 0; attempt < 12; attempt++) {
                const items = [...document.querySelectorAll(
                    ".cdk-overlay-container .mat-mdc-menu-content button, .cdk-overlay-container [role=menuitem], .cdk-overlay-pane button"
                )];
                const target = items.find(item => {
                    if (item.disabled || item.getAttribute("aria-disabled") === "true") return false;
                    const icon = item.querySelector("mat-icon, .material-symbols-outlined, [data-mat-icon-name]");
                    const label = [
                        item.getAttribute("aria-label"),
                        icon?.getAttribute("fonticon"),
                        icon?.getAttribute("data-mat-icon-name"),
                        icon?.textContent,
                        item.textContent
                    ].filter(Boolean).join(" ").toLowerCase();
                    return /(?:delete|trash|删除|移除)/i.test(label);
                });
                if (target) return target;
                await sleep(80);
            }
            return null;
        }

        async function deleteOne(id) {
            const turn = document.getElementById(id);
            if (!turn || !turn.isConnected) return;
            const button = turn.querySelector("ms-chat-turn-options span.ms-button-icon-symbol") ||
                turn.querySelector("ms-chat-turn-options button.mat-mdc-menu-trigger") ||
                turn.querySelector("ms-chat-turn-options button");
            if (!button) {
                if (normalizeType(turn) === "thought") {
                    throw new Error("思考块 " + id + " 尚未随回复被删除，也没有独立的原生删除菜单");
                }
                throw new Error("消息 " + id + " 缺少原生操作菜单");
            }
            button.click();
            const item = await findNativeDeleteItem();
            if (!item) throw new Error("找不到 Google 原生删除选项，已停止");
            item.focus?.();
            item.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true }));
            item.click();
            // A further confirmation dialog is deliberately not bypassed.
            for (let i = 0; i < 100; i++) {
                if (!document.getElementById(id)) return;
                await sleep(100);
            }
            throw new Error("消息 " + id + " 删除后未从页面消失，已停止");
        }

        async function commit() {
            if (deleting) return;
            const plan = pending;
            if (!plan) throw new Error("没有可应用的摘要");
            if (Branching.getCurrentPromptId() !== plan.promptId) {
                throw new Error("会话已切换，请重新读取摘要");
            }
            const baseline = await verifyPersistedSummary(plan);
            const latest = findSavedSummary(baseline.turns, plan.promptId);
            const original = baseline.turns.slice(0, latest.lastUserIndex);
            const recent = RoundSplitter.partition(original, plan.recentRounds).recent;
            const expectedProtected = JSON.stringify(recent.map(turn => [turn.role, turn.text]));
            const preview = preflightDom(plan);
            const expectedUsers = baseline.turns.filter(turn => turn.role === "user").length -
                preview.deletedUserCount;
            const prompt = "准备一次性清理 " + preview.deletedOldRounds + " 轮旧历史" +
                (preview.pinnedCount ? "（已保护 " + preview.pinnedCount + " 条旧压缩记忆）" : "") +
                (plan.memoryMarker ? "，并清理本次工具指令及可独立删除的思考块" : "") +
                "。保留最近 " + plan.recentRounds + " 轮与已标记摘要。" +
                (preview.skippedThoughts ? "另有 " + preview.skippedThoughts +
                    " 个无法独立删除的思考块会保留。" : "") +
                "所有页面删除完成后只进行一次云端保存验证。该操作不可撤销，继续？";
            if (!confirm(prompt)) return;
            deleting = true;
            state.branchInProgress = true;
            let processed = 0;
            const saveBaseline = SaveMonitor.attempts;
            try {
                const protectedSet = new Set(preview.protectedIds);
                for (const id of preview.targetIds) {
                    if (Branching.getCurrentPromptId() !== plan.promptId) {
                        throw new Error("清理过程中会话已经切换");
                    }
                    await deleteOne(id);
                    processed++;
                    // An Angular cascading delete may remove the summary
                    // alongside its temporary user prompt. Stop immediately
                    // if that happens, instead of deleting the rest blindly.
                    for (const protectedId of protectedSet) {
                        if (!document.getElementById(protectedId)) {
                            throw new Error("原生删除联动影响了受保护消息 " +
                                protectedId + "，已立即停止。请刷新并核对摘要");
                        }
                    }
                    PromptPanel.status("批量清理中 " + processed + "/" + preview.items +
                        "，完成后统一验证 Google 保存");
                }
                PromptPanel.status("本批次已在页面处理，正在统一核对 Google 云端……");
                await awaitCloudPersistence(plan, expectedUsers, expectedProtected, preview, saveBaseline);
                pending = null;
                PromptPanel.showCompactionReview(null);
                PromptPanel.status("Google 云端已核对：旧历史清理完成，标记的压缩记忆和最近轮次仍在。" +
                    (preview.skippedThoughts ? "部分内嵌思考块因缺少独立删除入口而保留。" : ""));
            } catch (error) {
                PromptPanel.status("批量清理处理了 " + processed + "/" + preview.items +
                    " 个页面消息后停止：" + String(error?.message || error) +
                    "。请刷新检查服务器实际状态");
                throw error;
            } finally {
                deleting = false;
                state.branchInProgress = false;
            }
        }

        function cancel() {
            if (deleting) return;
            pending = null;
            PromptPanel.showCompactionReview(null);
            PromptPanel.status("已取消清理。摘要已生成在当前聊天中，原始历史仍在。");
        }

        return Object.freeze({ stage, commit, cancel, collectGroups, preflightDom, get pending() { return pending; } });
    })();

    const UI = {
        init() {
            UI.injectButton();

            window.addEventListener("DOMContentLoaded", () => {
                log.ui("DOMContentLoaded");
                UI.injectButton();
            });

            window.addEventListener("load", () => {
                log.ui("window load event");
                UI.injectButton();
            });

            const observer = new MutationObserver(() => {
                UI.injectButton();
            });

            function startObserver() {
                let attached = false;
                for (const doc of UI.getCandidateDocuments()) {
                    if (doc.body) {
                        observer.observe(doc.body, { childList: true, subtree: true });
                        attached = true;
                    }
                }
                if (attached) {
                    log.ui("MutationObserver for button injection started");
                } else {
                    log.ui("Document body not ready for observer");
                }
            }

            if (document.readyState === "loading") {
                document.addEventListener("DOMContentLoaded", startObserver, { once: true });
            } else {
                startObserver();
            }

            setInterval(() => UI.injectButton(), 2000);
        },

        injectButton() {
            if (!window.location.href.includes(Config.UI.TARGET_PATH)) return;
            for (const doc of UI.getCandidateDocuments()) {
                if (doc.getElementById(Config.UI.BUTTON_ID)) return;
                const toolbar = UI.findToolbar(doc);
                if (!toolbar) continue;
                UI.toolbarMissingLogged = false;
                const button = UI.createButton(doc);
                const moreButton = toolbar.querySelector(
                    'button[iconname="more_vert"], button[aria-label="View more actions"], button[aria-label="More actions"], button[aria-label="More options"]'
                );
                let referenceNode = null;
                if (moreButton) {
                    if (moreButton.parentElement === toolbar) {
                        referenceNode = moreButton;
                    } else if (moreButton.parentElement && moreButton.parentElement.parentElement === toolbar) {
                        referenceNode = moreButton.parentElement;
                    }
                }
                if (referenceNode) {
                    toolbar.insertBefore(button, referenceNode);
                    log.ui("Compressor button inserted before menu button");
                } else {
                    toolbar.appendChild(button);
                    log.ui("Compressor button appended to toolbar");
                }
                return;
            }
            if (!UI.toolbarMissingLogged) {
                log.ui("Toolbar not found yet; waiting for AI Studio to load");
                UI.toolbarMissingLogged = true;
            }
        },
        findToolbar(doc) {
            const selectors = Array.isArray(Config.UI.TOOLBAR_SELECTORS)
                ? Config.UI.TOOLBAR_SELECTORS
                : [Config.UI.TOOLBAR_SELECTORS || Config.UI.TOOLBAR_SELECTOR].filter(Boolean);
            for (const selector of selectors) {
                const nodes = Array.from(doc.querySelectorAll(selector));
                for (const node of nodes) {
                    if (UI.isToolbarCandidate(node)) return node;
                }
            }
            return null;
        },
        isToolbarCandidate(node) {
            if (!node || typeof node.querySelector !== "function") return false;
            const hasMenuButton = node.querySelector(
                'button[iconname="more_vert"], button[aria-label="View more actions"], button[aria-label="More actions"], button[aria-label="More options"]'
            );
            if (hasMenuButton) return true;
            const hasRunSettings = node.querySelector('button[aria-label="Toggle run settings panel"]');
            if (hasRunSettings) return true;
            const hasNewChat = node.querySelector('button[aria-label="New chat"]');
            if (hasNewChat) return true;
            return false;
        },

        createButton(doc = document) {
            UI.ensureStyles(doc);
            const button = doc.createElement("button");
            button.id = Config.UI.BUTTON_ID;
            button.title = "Compress Chat History";
            button.setAttribute("ms-button", "");
            button.setAttribute("variant", "icon-borderless");
            button.setAttribute("mattooltip", "Compress Chat History");
            button.setAttribute("mattooltipposition", "below");
            button.setAttribute("iconname", "summarize");
            button.className = "mat-mdc-tooltip-trigger ms-button-borderless ms-button-icon";
            button.setAttribute("aria-label", "Compress Chat History");
            button.setAttribute("aria-disabled", "false");
            button.type = "button";
            button.addEventListener("click", Compression.run);

            const iconSpan = doc.createElement("span");
            iconSpan.className = "material-symbols-outlined notranslate ms-button-icon-symbol compressor-icon";
            iconSpan.setAttribute("aria-hidden", "true");
            iconSpan.textContent = "docs";
            button.appendChild(iconSpan);

            const spinnerSpan = doc.createElement("span");
            spinnerSpan.className = "compressor-spinner";
            spinnerSpan.setAttribute("aria-hidden", "true");
            button.appendChild(spinnerSpan);

            return button;
        },

        ensureStyles(doc) {
            const styleId = `${Config.UI.BUTTON_ID}-style`;
            if (doc.getElementById(styleId)) return;
            const style = doc.createElement("style");
            style.id = styleId;
            style.textContent = `@keyframes aistudio-compressor-spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
}
#${Config.UI.BUTTON_ID} {
    position: relative;
    width: 36px !important;
    min-width: 36px !important;
    height: 36px !important;
    padding: 6px !important;
    border-radius: 18px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: transparent;
}
#${Config.UI.BUTTON_ID} .compressor-spinner {
    box-sizing: border-box;
    width: 20px;
    height: 20px;
    border: 2px solid rgba(0, 0, 0, 0.2);
    border-top-color: currentColor;
    border-radius: 50%;
    animation: aistudio-compressor-spin 0.8s linear infinite;
    display: none;
}
#${Config.UI.BUTTON_ID}.compressor-loading .compressor-spinner {
    display: inline-flex;
    align-items: center;
    justify-content: center;
}
#${Config.UI.BUTTON_ID}.compressor-loading .compressor-icon {
    display: none;
}`;
            const target = doc.head || doc.documentElement || doc.body;
            if (target) {
                target.appendChild(style);
            }
        },

        findInput() {
            const docs = UI.getCandidateDocuments();
            for (const doc of docs) {
                for (const selector of Config.UI.INPUT_SELECTORS) {
                    const el = doc.querySelector(selector);
                    if (el) {
                        log.ui("Input box found via selector", selector);
                        return el;
                    }
                }
            }
            return null;
        },

        findSendButton() {
            const docs = UI.getCandidateDocuments();
            for (const doc of docs) {
                for (const selector of Config.UI.SEND_BUTTON_SELECTORS) {
                    const btn = doc.querySelector(selector);
                    if (btn) {
                        log.ui("Send button found via selector", selector);
                        return btn;
                    }
                }
            }
            return null;
        },

        getCandidateDocuments() {
            const docs = [document];
            const frameDoc = document.querySelector('iframe[src*="bscframe"]')?.contentDocument;
            if (frameDoc && frameDoc !== document) {
                docs.push(frameDoc);
            }
            return docs;
        },

        setButtonLoading(isLoading) {
            const button = UI.findButton();
            if (!button) return;
            if (isLoading) {
                button.classList.add("compressor-loading");
                button.setAttribute("disabled", "true");
                button.setAttribute("aria-disabled", "true");
            } else {
                button.classList.remove("compressor-loading");
                button.removeAttribute("disabled");
                button.removeAttribute("aria-disabled");
            }
        },

        findButton() {
            for (const doc of UI.getCandidateDocuments()) {
                const btn = doc.getElementById(Config.UI.BUTTON_ID);
                if (btn) return btn;
            }
            return null;
        }
    };

    // ---------------------------
    // Prompt thread builders
    // ---------------------------

    // CSS-only performance workaround inspired by xgloom/fix-aistudio-lag.
    // No removal, rewriting, or hiding of chat history. Toggle is reversible.
    const PerformanceTuning = (() => {
        const KEY = "sujiu:aistudio:reduce-visual-effects:v1";
        const STYLE_ID = "sujiu-aistudio-performance-style";
        const CSS = [
            "*, *::before, *::after {",
            "  backdrop-filter: none !important;",
            "  -webkit-backdrop-filter: none !important;",
            "  scroll-behavior: auto !important;",
            "}"
        ].join("\n");
        let enabled = true;
        try { enabled = localStorage.getItem(KEY) !== "false"; }
        catch {}

        function apply(doc = document) {
            const existing = doc.getElementById?.(STYLE_ID);
            if (!enabled) {
                existing?.remove();
                return false;
            }
            if (existing) return true;
            if (!doc.head) return false;
            const style = doc.createElement("style");
            style.id = STYLE_ID;
            style.textContent = CSS; // Trusted Types safe.
            doc.head.appendChild(style);
            return true;
        }

        function init() {
            apply();
            if (!document.head && document.readyState === "loading") {
                document.addEventListener("DOMContentLoaded", () => apply(), { once: true });
            }
        }

        function setEnabled(value) {
            enabled = Boolean(value);
            try { localStorage.setItem(KEY, String(enabled)); }
            catch {}
            apply();
            return enabled;
        }

        return Object.freeze({ init, apply, setEnabled, get enabled() { return enabled; } });
    })();

    // Passive observation only. Never replay or synthesize UpdatePrompt:
    // its internal schema changes and an invalid write can corrupt chat data.
    const SaveMonitor = (() => {
        let attempts = 0;
        let successes = 0;
        let last = null;
        function record(status, url = "") {
            if (!isTargetRequest(url, "UPDATE")) return;
            attempts++;
            const ok = Number(status) >= 200 && Number(status) < 300;
            if (ok) successes++;
            last = { ok, status: Number(status) || 0, time: new Date().toISOString() };
            const runtime = window.__SUJIU_AISTUDIO_RUNTIME__;
            if (runtime) runtime.save = { attempts, successes, ...last };
            log.net("Observed native Google UpdatePrompt result", {
                status: last.status, ok: last.ok
            });
        }
        return Object.freeze({
            record,
            get latest() { return last; },
            get attempts() { return attempts; }
        });
    })();

    function extractMutationPromptId(root) {
        const resourceName = typeof root?.[0] === "string" ? root[0] : "";
        const match = resourceName.match(/prompts\/([^/?#]+)/);
        return match ? match[1] : null;
    }

    function deepClone(value) {
        return value == null ? value : JSON.parse(JSON.stringify(value));
    }

    function safeParseJSON(text) {
        if (typeof text !== "string") return null;
        try {
            return JSON.parse(text);
        } catch {
            return null;
        }
    }

    function buildSnippetVariants(snippet) {
        if (typeof snippet !== "string" || !snippet) {
            return [];
        }
        const variants = [snippet];
        const escaped = escapeForJSON(snippet);
        if (escaped && escaped !== snippet) {
            variants.push(escaped);
        }
        return variants;
    }

    function escapeForJSON(text) {
        try {
            const encoded = JSON.stringify(text);
            return typeof encoded === "string" ? encoded.slice(1, -1) : text;
        } catch {
            return text;
        }
    }

    function matchByRegex(text, pattern) {
        if (!pattern) return null;
        try {
            const regex = new RegExp(pattern, "s");
            const result = text.match(regex);
            return result ? result[0] : null;
        } catch (err) {
            log.net("Invalid snapshot regex", err);
            return null;
        }
    }

const PromptProfiles = (() => {
    const sections = Object.freeze([
      { id: "system", title: "系统提示词" },
      { id: "character", title: "角色设定" },
      { id: "world", title: "世界设定" },
      { id: "persona", title: "用户人物卡" },
      { id: "author", title: "作者附注" }
    ]);
    const ids = sections.map(section => section.id);
    const idsSet = new Set(ids);
    const RESERVED = new Set(["__proto__", "prototype", "constructor"]);
    const MAX_PRESETS = 50;
    const MAX_FIELD_LENGTH = 100000;
    function blank() {
      return {
        fields: Object.fromEntries(ids.map(id => [id, ""])),
        enabled: Object.fromEntries(ids.map(id => [id, true])),
        order: ids.slice()
      };
    }
    function normalizedPreset(input) {
      const preset = blank();
      if (!input || typeof input !== "object" || Array.isArray(input)) return preset;
      // Read legacy presets where text fields were stored directly on the preset.
      const fields = input.fields && typeof input.fields === "object" ? input.fields : input;
      for (const id of ids) {
        if (typeof fields[id] === "string") preset.fields[id] = fields[id].slice(0, MAX_FIELD_LENGTH);
        if (typeof input.enabled?.[id] === "boolean") preset.enabled[id] = input.enabled[id];
      }
      if (Array.isArray(input.order)) {
        const order = [...new Set(input.order.filter(id => idsSet.has(id)))];
        preset.order = [...order, ...ids.filter(id => !order.includes(id))];
      }
      return preset;
    }
    function normalize(source) {
      const presets = Object.create(null);
      if (source && typeof source === "object" && !Array.isArray(source) &&
          source.presets && typeof source.presets === "object" && !Array.isArray(source.presets)) {
        for (const [rawName, rawPreset] of Object.entries(source.presets)) {
          const name = rawName.trim().slice(0, 80);
          if (!name || RESERVED.has(name) || Object.hasOwn(presets, name)) continue;
          presets[name] = normalizedPreset(rawPreset);
          if (Object.keys(presets).length >= MAX_PRESETS) break;
        }
      }
      if (!Object.keys(presets).length) presets["默认"] = blank();
      const active = typeof source?.active === "string" && Object.hasOwn(presets, source.active)
        ? source.active : Object.keys(presets)[0];
      return { version: 2, active, presets };
    }
    function compile(preset) {
      const p = normalizedPreset(preset);
      return p.order.filter(id => p.enabled[id] && p.fields[id].trim())
        .map(id => "[" + sections.find(s => s.id === id).title + "]\n" + p.fields[id].trim())
        .join("\n\n");
    }
    function shift(preset, id, delta) {
      const p = normalizedPreset(preset);
      const at = p.order.indexOf(id);
      const next = at + delta;
      if (at >= 0 && next >= 0 && next < p.order.length) {
        [p.order[at], p.order[next]] = [p.order[next], p.order[at]];
      }
      return p;
    }
    return Object.freeze({ sections, blank, normalize, normalizedPreset, compile, shift, maxPresets: MAX_PRESETS });
  })();

const PromptPanel = (() => {
    const STORAGE_V2 = "sujiu:aistudio:prompt-presets:v2";
    const STORAGE_V1 = "sujiu:aistudio:prompt-presets:v1";
    function load() {
      try {
        const raw = localStorage.getItem(STORAGE_V2) || localStorage.getItem(STORAGE_V1);
        return PromptProfiles.normalize(raw ? JSON.parse(raw) : null);
      } catch {
        return PromptProfiles.normalize(null);
      }
    }
    let data = load();
    let root = null;
    let panel = null;
    let lastAutoApplied = "";
    function save() {
      try {
        localStorage.setItem(STORAGE_V2, JSON.stringify(data));
      } catch (error) {
        status("预设保存失败：" + String(error.message || error));
      }
    }
    function current() {
      if (!Object.hasOwn(data.presets, data.active)) data = PromptProfiles.normalize(data);
      return data.presets[data.active];
    }
    const combined = () => PromptProfiles.compile(current());
    function showCompactionReview(plan) {
      const container = root?.getElementById("inplace-review");
      if (!container) return;
      container.hidden = !plan;
      if (plan) {
        root.querySelector('[data-role="snapshot-preview"]').value = plan.snapshot;
        const warning = plan.legacy
          ? "这是旧版摘要，可能把过去剧情写成当前状态；建议重新生成后再清理。"
          : "请确认已结束事件、最新剧情时间线和摘要内容。";
        status("已读取摘要：" + plan.totalRounds + " 轮中，拟删除旧 " +
          plan.olderRounds + " 轮、保留最近 " + plan.recentRounds + " 轮。" + warning);
      }
    }
    function status(message) {
      const el = root?.querySelector('[data-role="status"]');
      if (el) el.textContent = String(message);
    }
    function nativeField() {
      const selectors = [
        'ms-system-instructions textarea',
        'ms-system-instruction-editor textarea',
        'textarea[aria-label*="System instructions"]',
        'textarea[aria-label*="System Instructions"]',
        'textarea[aria-label*="系统指令"]',
        '[aria-label="System instructions"] [contenteditable="true"]',
        'ms-system-instructions [contenteditable="true"]'
      ];
      for (const doc of UI.getCandidateDocuments()) {
        for (const selector of selectors) {
          const el = doc.querySelector(selector);
          if (el && !el.disabled && !el.readOnly) return el;
        }
      }
      return null;
    }
    function applyNative(text) {
      const el = nativeField();
      if (!el || !text) return false;
      if (el.tagName === "TEXTAREA") {
        if (el.value === text) return true;
        const proto = el.ownerDocument.defaultView?.HTMLTextAreaElement?.prototype || HTMLTextAreaElement.prototype;
        const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
        if (!setter) return false;
        setter.call(el, text);
      } else if (el.isContentEditable) {
        if (el.textContent === text) return true;
        el.textContent = text;
      } else return false;
      el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    }
    function autoInjectOnce() {
      if (!userSettings.autoInject || state.compressionInProgress || state.branchInProgress) return;
      const text = combined();
      if (!text) return;
      const token = location.pathname + ":" + data.active + ":" + text;
      if (lastAutoApplied === token) return;
      if (applyNative(text)) {
        lastAutoApplied = token;
        status("已尝试同步系统指令；请确认 AI Studio 已保存");
      }
    }
    async function clipboard(text) {
      try {
        await navigator.clipboard.writeText(text);
        return true;
      } catch { return false; }
    }
    function renderFields() {
      if (!root) return;
      const profile = current();
      const container = root.getElementById("fields");
      for (const id of profile.order) {
        const row = root.querySelector('[data-section="' + id + '"]');
        row.querySelector('[data-enable="' + id + '"]').checked = profile.enabled[id] !== false;
        row.querySelector('[data-field="' + id + '"]').value = profile.fields[id] || "";
        container.appendChild(row);
      }
    }
    function render() {
      if (!root) return;
      const select = root.querySelector('[data-role="preset"]');
      select.replaceChildren();
      for (const name of Object.keys(data.presets)) {
        const option = document.createElement("option");
        option.value = name;
        option.textContent = name;
        select.appendChild(option);
      }
      select.value = data.active;
      renderFields();
      root.querySelector('[data-role="keep"]').value = String(userSettings.keepRounds || 8);
      root.querySelector('[data-role="auto-inject"]').checked = Boolean(userSettings.autoInject);
      root.querySelector('[data-role="reduce-visual-effects"]').checked = PerformanceTuning.enabled;
      root.querySelector('[data-role="summary-prompt"]').value = getCompressPrompt();
    }
    function exportPresets() {
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      try {
        const link = document.createElement("a");
        link.href = url;
        link.download = "sujiu-aistudio-presets.json";
        document.body.appendChild(link);
        link.click();
        link.remove();
        status("预设已导出");
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 5000);
      }
    }
    async function importPresets(file) {
      if (!file || file.size > 1024 * 1024) {
        status("导入失败：文件超过 1 MB 或未选择文件");
        return;
      }
      try {
        const raw = JSON.parse(await file.text());
        if (!raw || !raw.presets || typeof raw.presets !== "object" || Array.isArray(raw.presets) ||
            Object.keys(raw.presets).length === 0) throw new Error("预设结构不正确");
        if (!confirm("导入将替换现有预设。建议先导出备份，是否继续？")) return;
        data = PromptProfiles.normalize(raw);
        save();
        lastAutoApplied = "";
        render();
        status("已导入 " + Object.keys(data.presets).length + " 个预设");
      } catch (error) {
        status("预设导入失败：" + String(error?.message || error));
      }
    }
    function mount() {
      if (document.getElementById("sujiu-aistudio-host")) return;
      // At document-start the body may not exist yet; wait for it rather
      // than trying to attach a fixed overlay to an incomplete DOM.
      if (!document.body) {
        if (document.readyState === "loading") {
          document.addEventListener("DOMContentLoaded", mount, { once: true });
        }
        if (!mount.retryScheduled) {
          mount.retryScheduled = true;
          const retry = setInterval(() => {
            if (!document.body) return;
            clearInterval(retry);
            mount.retryScheduled = false;
            mount();
          }, 250);
        }
        return;
      }
      const host = document.createElement("div");
      host.id = "sujiu-aistudio-host";
      host.style.cssText = "position:fixed;right:18px;bottom:18px;z-index:2147483000";
      const shadow = host.attachShadow({ mode: "open" });
      // AI Studio enforces Trusted Types; construct the panel with DOM APIs.
      // Do not assign strings to innerHTML / insertAdjacentHTML / document.write.
      const make = (tag, attributes = {}, children = []) => {
        const element = document.createElement(tag);
        for (const [name, value] of Object.entries(attributes)) {
          if (name === "text") element.textContent = String(value);
          else if (name === "className") element.className = String(value);
          else if (name === "hidden") element.hidden = Boolean(value);
          else element.setAttribute(name, String(value));
        }
        element.append(...children);
        return element;
      };
      const style = make("style", { text: "\n          *{box-sizing:border-box} button,input,select,textarea{font:inherit}\n          button{cursor:pointer;border:1px solid #64748b66;border-radius:8px;padding:6px 8px;background:#ffffff12;color:inherit}\n          button:hover{background:#94a3b833}button:focus-visible,input:focus-visible,select:focus-visible,textarea:focus-visible{outline:2px solid #0d9488}\n          #toggle{background:#156f67;color:white;border:0;box-shadow:0 3px 16px #0005}\n          #pane{width:min(405px,calc(100vw - 24px));max-height:min(82vh,850px);margin-bottom:9px;overflow:auto;color:#f1f5f9;background:#1b2029;border:1px solid #64748b55;border-radius:14px;padding:15px;box-shadow:0 6px 40px #0008;font:13px/1.5 system-ui,sans-serif}\n          #pane[hidden]{display:none}h2{font-size:15px;margin:0 0 10px}h3{font-size:13px;margin:16px 0 7px}.line{display:flex;align-items:center;gap:6px;margin:6px 0}.line>*{min-width:0}\n          select,input[type=number],textarea{width:100%;background:#0f172a;color:#f8fafc;border:1px solid #64748b99;border-radius:8px;padding:7px}\n          textarea{display:block;resize:vertical;min-height:62px;margin:5px 0 10px}\n          label{font-size:12px;color:#cbd5e1}.section-header{display:flex;align-items:center;gap:7px;justify-content:space-between}\n          .section-header label{flex:1}.small{padding:3px 7px;font-size:12px}\n          #status{color:#99f6e4;min-height:1.5em;font-size:12px;margin-top:10px}\n          .hint{color:#94a3b8;font-size:11px;margin:7px 0}\n          .check-line{display:flex;gap:8px;align-items:center}.check-line input{width:auto}\n          hr{border:0;border-top:1px solid #64748b55;margin:14px 0}\n          #inplace-review[hidden]{display:none}\n        " });
      const line = (...children) => make("div", { className: "line" }, children);
      const action = (label, name) => make("button", { type: "button", "data-action": name, text: label });
      const role = (name, tag = "input", attributes = {}) =>
        make(tag, { ...attributes, "data-role": name });
      const heading = make("h2", { text: "素酒 · AI Studio" });
      const pane = make("section", { id: "pane", hidden: true, "aria-label": "素酒 AI Studio 工具" }, [
        heading,
        make("label", { text: "提示词预设" }),
        line(role("preset", "select", { "aria-label": "当前预设" }), action("新建", "new"), action("删除", "delete")),
        line(action("导入", "import"), action("导出", "export")),
        role("file", "input", { type: "file", accept: ".json,application/json", hidden: true }),
        make("div", { id: "fields" }),
        line(action("注入系统指令", "apply"), action("复制组合内容", "copy")),
        make("label", { className: "check-line" }, [
          role("auto-inject", "input", { type: "checkbox" }),
          document.createTextNode(" 自动同步提示词到系统指令框")
        ]),
        make("p", { className: "hint", text: "按勾选和顺序合并所有提示词；自动同步仅在 AI Studio 的原生编辑框可访问时执行。" }),
        make("hr"),
        make("h3", { text: "性能" }),
        make("label", { className: "check-line" }, [
          role("reduce-visual-effects", "input", { type: "checkbox" }),
          document.createTextNode(" 减少页面特效（模糊与平滑滚动）")
        ]),
        make("p", { className: "hint", text: "可减少 GPU 重绘引起的卡顿；不会删减消息，也无法解决超长对话的全部 DOM 和 token 计算开销。" }),
        make("h3", { text: "上下文管理" }),
        make("label", { text: "保留最近完整轮数" }),
        role("keep", "input", { type: "number", min: "1", max: "100", step: "1" }),
        make("label", { text: "压缩摘要提示词" }),
        role("summary-prompt", "textarea", { rows: "5" }),
        line(action("生成摘要（当前聊天）", "compress")),
        make("p", { className: "hint", text: "生成没有固定等待时限。模型完成回复后，再点击读取摘要；两项操作相互独立。" }),
        line(action("读取已完成摘要", "recover-summary"), action("停止插件监听", "stop-tracking")),
        make("div", { id: "inplace-review", hidden: true }, [
          make("label", { text: "摘要预览（已写入原聊天）" }),
          role("snapshot-preview", "textarea", { rows: "8", readonly: "readonly" }),
          line(action("确认删除较早的聊天和思考", "commit-delete"),
               action("保留原历史", "cancel-delete"))
        ]),
        make("p", { className: "hint", text: "摘要生成后需再次确认才会删除早期轮次和关联思考。直接修改当前聊天，无新分支，也无法撤销。请先在测试会话使用。" }),
        make("div", { id: "status", "data-role": "status", role: "status", "aria-live": "polite" })
      ]);
      const toggle = make("button", { id: "toggle", type: "button",
        "aria-expanded": "false", "aria-controls": "pane", text: "素酒工具" });
      shadow.append(style, pane, toggle);
      // Inline style fallback if the host blocks injecting a <style> element.
      pane.style.width = "min(405px, calc(100vw - 24px))";
      pane.style.maxHeight = "min(82vh, 850px)";
      pane.style.overflow = "auto";
      pane.style.backgroundColor = "#1b2029";
      pane.style.color = "#f1f5f9";
      pane.style.padding = "15px";
      pane.style.borderRadius = "14px";
      toggle.style.backgroundColor = "#156f67";
      toggle.style.color = "white";
      toggle.style.padding = "8px 12px";
      toggle.style.borderRadius = "9px";
      toggle.style.cursor = "pointer";
      root = shadow;
      panel = shadow.getElementById("pane");
      const container = shadow.getElementById("fields");
      for (const section of PromptProfiles.sections) {
        const row = document.createElement("div");
        row.dataset.section = section.id;
        const head = document.createElement("div");
        head.className = "section-header";
        const label = document.createElement("label");
        const enable = document.createElement("input");
        enable.type = "checkbox";
        enable.dataset.enable = section.id;
        label.append(enable, document.createTextNode(" " + section.title));
        const up = document.createElement("button");
        up.textContent = "↑";
        up.className = "small";
        up.title = "上移" + section.title;
        up.dataset.move = section.id;
        up.dataset.delta = "-1";
        const down = document.createElement("button");
        down.textContent = "↓";
        down.className = "small";
        down.title = "下移" + section.title;
        down.dataset.move = section.id;
        down.dataset.delta = "1";
        head.append(label, up, down);
        const editor = document.createElement("textarea");
        editor.dataset.field = section.id;
        editor.rows = 3;
        editor.setAttribute("aria-label", section.title);
        row.append(head, editor);
        container.appendChild(row);
      }
      shadow.getElementById("toggle").addEventListener("click", () => {
        panel.hidden = !panel.hidden;
        shadow.getElementById("toggle").setAttribute("aria-expanded", String(!panel.hidden));
        if (!panel.hidden) render();
      });
      shadow.querySelector('[data-role="preset"]').addEventListener("change", event => {
        data.active = event.target.value;
        save(); lastAutoApplied = ""; render();
      });
      shadow.querySelector('[data-role="file"]').addEventListener("change", event => {
        void importPresets(event.target.files?.[0]);
        event.target.value = "";
      });
      shadow.addEventListener("input", event => {
        const id = event.target?.dataset?.field;
        if (id) {
          current().fields[id] = event.target.value.slice(0, 100000);
          save(); lastAutoApplied = "";
        }
      });
      shadow.addEventListener("change", event => {
        const el = event.target;
        const id = el?.dataset?.enable;
        if (id) {
          current().enabled[id] = el.checked;
          save(); lastAutoApplied = "";
          return;
        }
        const role = el?.dataset?.role;
        if (role === "reduce-visual-effects") {
          const enabled = PerformanceTuning.setEnabled(el.checked);
          status(enabled ? "已减少页面模糊与平滑滚动，可随时关闭" : "已恢复网页原有视觉效果");
        }
        if (role === "auto-inject") {
          updateUserSettings({ autoInject: el.checked });
          lastAutoApplied = "";
          autoInjectOnce();
        }
        if (role === "keep") {
          const keep = Number(el.value);
          if (!Number.isSafeInteger(keep) || keep < 1 || keep > 100) {
            status("保留轮数无效：请输入 1–100 之间的整数");
            render();
          } else {
            updateUserSettings({ keepRounds: keep });
            status("压缩将保留最近 " + keep + " 轮完整对话");
          }
        }
        if (role === "summary-prompt") {
          const value = el.value.trim();
          if (value) {
            updateUserSettings({ compressPrompt: value });
            refreshActivePromptFromSettings();
            status("压缩摘要指令已保存");
          }
        }
      });
      shadow.addEventListener("click", async event => {
        const moveId = event.target?.dataset?.move;
        if (moveId) {
          data.presets[data.active] = PromptProfiles.shift(current(), moveId, Number(event.target.dataset.delta));
          save(); lastAutoApplied = ""; renderFields();
          return;
        }
        const action = event.target?.dataset?.action;
        if (!action) return;
        if (action === "new") {
          const name = prompt("新预设名称");
          if (!name?.trim()) return;
          const cleaned = name.trim().slice(0, 80);
          if (["__proto__", "prototype", "constructor"].includes(cleaned) ||
              Object.hasOwn(data.presets, cleaned) || Object.keys(data.presets).length >= PromptProfiles.maxPresets) {
            status("预设名称不可用、已存在，或已达到 50 个上限");
            return;
          }
          data.presets[cleaned] = PromptProfiles.normalizedPreset(current());
          data.active = cleaned;
          save(); lastAutoApplied = ""; render(); status("已创建预设");
        }
        if (action === "delete") {
          if (Object.keys(data.presets).length <= 1) { status("至少保留一个预设"); return; }
          if (!confirm("删除预设「" + data.active + "」？")) return;
          delete data.presets[data.active];
          data.active = Object.keys(data.presets)[0];
          save(); lastAutoApplied = ""; render(); status("已删除预设");
        }
        if (action === "export") exportPresets();
        if (action === "import") root.querySelector('[data-role="file"]').click();
        if (action === "copy" || action === "apply") {
          const text = combined();
          if (!text) { status("至少需要一个已启用且有内容的提示词框"); return; }
          if (action === "apply" && applyNative(text)) {
            lastAutoApplied = location.pathname + ":" + data.active + ":" + text;
            status("已尝试填入系统指令，请确认 AI Studio 是否保存");
            return;
          }
          const ok = await clipboard(text);
          status(ok ? "已复制提示词，可粘贴至系统指令框" : "复制失败：请检查剪贴板权限");
        }
        if (action === "compress") {
          status("已开始生成摘要。模型完成后请点击「读取已完成摘要」；不会自动删除任何历史消息");
          Compression.run();
        }
        if (action === "recover-summary") await Compression.recover();
        if (action === "stop-tracking") Compression.stopTracking();
        if (action === "commit-delete") {
          try { await InPlaceCompaction.commit(); }
          catch (error) {
            const reason = String(error?.message || error);
            status("删除未完成：" + reason);
            console.warn("[Sujiu AI Studio] in-place deletion halted", reason);
          }
        }
        if (action === "cancel-delete") InPlaceCompaction.cancel();
      });
      // Open once after a fresh install so users can confirm injection
      // without having to find the small bottom-right launcher.
      try {
        const seenKey = "sujiu:aistudio:panel-opened:v1";
        if (!localStorage.getItem(seenKey)) {
          panel.hidden = false;
          shadow.getElementById("toggle").setAttribute("aria-expanded", "true");
          localStorage.setItem(seenKey, "1");
        }
      } catch {
        panel.hidden = false;
        shadow.getElementById("toggle").setAttribute("aria-expanded", "true");
      }
      document.body.appendChild(host);
      console.info("[Sujiu AI Studio] prompt panel mounted");
      render();
      setInterval(autoInjectOnce, 1800);
    }
    return { mount, status, showCompactionReview };
  })();

    function bootstrap() {
        const runtimeInfo = {
            version: SCRIPT_VERSION,
            href: location.href,
            loadedAt: new Date().toISOString(),
            panelInitialized: false,
            networkInitialized: false,
            errors: []
        };
        // Stable diagnostic marker to distinguish a missing userscript permission
        // from a runtime initialization failure in the developer console.
        window.__SUJIU_AISTUDIO_RUNTIME__ = runtimeInfo;
        console.info("[Sujiu AI Studio] userscript started", runtimeInfo.version, runtimeInfo.href);

        function attempt(stage, action) {
            try {
                action();
                return true;
            } catch (error) {
                const message = String(error?.message || error);
                runtimeInfo.errors.push({ stage, message });
                console.error("[Sujiu AI Studio] " + stage + " initialization failed:", error);
                return false;
            }
        }

        // The panel is independent of AI Studio's private RPCs and fragile
        // toolbar selectors; initialize it before any hooks.
        attempt("performance", () => PerformanceTuning.init());
        runtimeInfo.panelInitialized = attempt("panel", () => PromptPanel.mount());
        attempt("events", installTopLevelHandlers);
        runtimeInfo.networkInitialized = attempt("network", () => NetworkMonitor.init());
        if (!env.isAppContext) attempt("toolbar", () => UI.init());
    }

    bootstrap();

bootstrapStatus.phase = "ready";
} catch (error) {
  bootstrapStatus.phase = "error";
  bootstrapStatus.error = String(error?.message || error);
  console.error("[Sujiu AI Studio] startup failure", error);
  const renderFailure = () => {
    if (!document.body || document.getElementById("sujiu-startup-error")) return;
    const notice = document.createElement("button");
    notice.id = "sujiu-startup-error";
    notice.type = "button";
    notice.textContent = "素酒启动失败 · 点击查看原因";
    notice.title = bootstrapStatus.error;
    notice.style.cssText = "position:fixed;right:16px;bottom:16px;z-index:2147483647;padding:12px;border:0;border-radius:10px;background:#a52a2a;color:white;cursor:pointer";
    notice.addEventListener("click", () => alert("素酒启动失败：" + bootstrapStatus.error));
    document.body.appendChild(notice);
  };
  if (document.body) renderFailure();
  else document.addEventListener("DOMContentLoaded", renderFailure, { once: true });
}
})();
