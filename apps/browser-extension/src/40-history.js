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
        if (!stored || !stored.fullHistoryVerified || task.marker !== stored.marker) {
            throw new Error("无法确认本次摘要请求包含全部历史；请重新生成摘要");
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
            throw new Error("Google 保存的聊天轮数与摘要生成时不一致，请检查是否新增或删除了消息");
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
