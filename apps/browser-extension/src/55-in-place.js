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

        // Explicit user/model markers alone decide protection.
        function isProtectedTurn(turn) {
            const role = normalizeType(turn);
            // Scope-tagged USER messages are the plugin's own temporary
            // inputs. They are never protected by their internal "sujiu"
            // token (or by archived history quoted in their body).
            if (role === "user" && temporaryTaskId(turn)) return false;
            return (role === "user" || role === "model") &&
                CompactionTask.isProtectedMessage(String(turn.textContent || ""));
        }
        function isArchiveReply(turn) {
            return normalizeType(turn) === "model" && isProtectedTurn(turn);
        }
        function isArchiveRequest(turn) {
            return Boolean(temporaryTaskId(turn));
        }
        function isArchiveGroup(group) {
            return group?.kind === "round" &&
                (group.turns.some(isProtectedTurn) ||
                    isArchiveRequest(group.turns[0]));
        }

        // The USER scope marker is a disposable plugin-generated task.
        // Only the MODEL memory marker denotes a persistent summary.
        function temporaryTaskId(turn) {
            if (normalizeType(turn) !== "user") return null;
            const match = String(turn.textContent || "").match(
                /\[\[SUJIU_SCOPE_V2:([a-z0-9-]{8,80})\]\]/i);
            return match ? match[1].toLowerCase() : null;
        }

        function temporaryTasks(turns) {
            const tasks = [];
            for (let index = 0; index < turns.length; index++) {
                const prompt = turns[index];
                const id = temporaryTaskId(prompt);
                if (!id) continue;
                const replies = [];
                for (let next = index + 1; next < turns.length; next++) {
                    const candidate = turns[next];
                    const type = normalizeType(candidate);
                    if (type === "user") break;
                    if (type === "model") replies.push(candidate);
                }
                // Any MODEL response following the explicit script task is
                // potentially a precious summary, even when its marker was
                // stripped by rendering. Never gamble with its native delete.
                tasks.push({ prompt, replies, reply: replies[0] || null });
            }
            return tasks;
        }

        function pairedArchiveIds(turns) {
            return new Set(temporaryTasks(turns)
                .flatMap(task => task.replies.map(reply => reply.id)));
        }

        function safeDisposablePrompts(turns) {
            return temporaryTasks(turns)
                // Google native USER deletion has destroyed its paired MODEL
                // reply in real use. An associated MODEL makes the scope USER
                // unsafe to remove with the native menu: preserve for now.
                .filter(task => task.replies.length === 0)
                .filter(task => hasIndependentDeleteMenu(task.prompt))
                .map(task => task.prompt);
        }

        // Stable memory markers live in Google-saved message text. We cannot
        // assume AI Studio persists custom private message metadata. Build a
        // full archive inventory from the saved transcript, not just the
        // newest compression task kept in localStorage.
        function archiveInventory(turns, roleOf, textOf) {
            const markers = new Map();
            let protectedCount = 0;
            let compressionRequests = 0;
            for (const turn of turns) {
                const role = roleOf(turn);
                const text = String(textOf(turn) || "");
                if (role !== "user" && role !== "model") continue;
                if (role === "user" &&
                    RoundSplitter.isCompressionPrompt({ role, text })) {
                    compressionRequests++;
                }
                if (!(role === "user" &&
                    RoundSplitter.isCompressionPrompt({ role, text })) &&
                    CompactionTask.isProtectedMessage(text)) protectedCount++;
                if (role === "model") {
                    const ids = new Set(CompactionTask.getMemoryMarkers(text)
                        .map(marker => marker.toLowerCase()));
                    for (const marker of ids) {
                        markers.set(marker, (markers.get(marker) || 0) + 1);
                    }
                }
            }
            return { markers, protectedCount, legacyReplies: 0,
                compressionRequests };
        }

        function verifyArchiveInventory(expected, actual) {
            const allMarkersMatch = expected.markers.size === actual.markers.size &&
                [...expected.markers].every(([marker, count]) =>
                    actual.markers.get(marker) === count);
            // Disposable task markers may disappear, protection may not.
            if (!allMarkersMatch ||
                expected.protectedCount !== actual.protectedCount) {
                throw new Error("Google 保存的历史压缩摘要记录与此前读取结果不一致");
            }
        }

        async function readSavedArchiveInventory(promptId, allowOpaque = false) {
            const capture = await fetchResolvedPromptThread(promptId);
            if (!capture?.entries?.length) {
                throw new Error("无法读取 Google 云端完整历史，拒绝清理");
            }
            const turns = extractResolvedTurns(capture.entries);
            // Native search/grounding and failed tool calls can contribute
            // opaque entries. Thought-only cleanup does not modify them, so
            // requiring a two-role transcript here would block valid chats.
            if (!allowOpaque) assertSupportedEntries(capture.entries, turns);
            const inventory = archiveInventory(turns,
                turn => turn.role === "model" && isThoughtOnlyTurn(turn) ?
                    "thought" : turn.role,
                turn => turn.text);
            // A thought-only cleanup must not silently lose a normal reply
            // that happens to lack a Sujiu-specific memory marker.
            inventory.protectedMessages = allowOpaque
                ? capture.entries
                    .filter(entry => Array.isArray(entry) && entry.length >= 9)
                    .filter(entry => !isThoughtOnlyTurn({
                        role: entry[8], entry
                    }))
                    .filter(entry => !(entry[8] === "user" &&
                        /\[\[SUJIU_SCOPE_V2:[a-z0-9-]{8,80}\]\]/i.test(
                            String(extractResolvedEntryText(entry) || ""))))
                    .map(entry => {
                        const text = extractResolvedEntryText(entry);
                        // Preserve unrecognized tool messages and error
                        // artifacts as opaque records, not as deletable turns.
                        return [String(entry[8] ?? "unknown"),
                            text || JSON.stringify(entry)];
                    })
                : turns
                    .filter(turn => !isThoughtOnlyTurn(turn))
                    .map(turn => [turn.role, String(turn.text || "")]);
            return inventory;
        }

        function verifyProtectedMessages(before, after) {
            if (JSON.stringify(before.protectedMessages) !==
                JSON.stringify(after.protectedMessages)) {
                throw new Error("Google 保存的普通对话或摘要记录与此前读取结果不一致");
            }
        }

        function hasIndependentDeleteMenu(turn) {
            return Boolean(turn.querySelector(
                "ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"
            ));
        }

        // Use every rendered native turn, including thought blocks. An
        // incomplete / virtualized DOM is never safe for destructive changes.
        function collectGroups(turns) {
            const groups = [];
            for (const turn of turns) {
                const type = normalizeType(turn);
                if (type === "unknown") {
                    groups.push({ turns: [turn], hasModel: false, kind: "opaque" });
                    continue;
                }
                if (type === "user") {
                    groups.push({ turns: [turn], hasModel: false, kind: "round" });
                    continue;
                }
                const group = groups[groups.length - 1];
                const isModel = type === "model";
                const separateModel = isModel && (!group ||
                    (group.hasModel && !isArchiveRequest(group.turns[0])));
                if (separateModel) {
                    // A separate MODEL turn is protected only if it has a marker.
                    groups.push({ turns: [turn], hasModel: true,
                        kind: isProtectedTurn(turn) ? "memory" : "round" });
                } else if (!group || group.kind === "opaque") {
                    groups.push({ turns: [turn], hasModel: isModel,
                        kind: "round" });
                } else {
                    group.turns.push(turn);
                    if (isModel) {
                        group.hasModel = true;
                        if (group.turns[0] &&
                            normalizeType(group.turns[0]) === "thought" &&
                            isProtectedTurn(turn)) {
                            group.kind = "memory";
                        }
                    }
                }
            }
            // Incomplete turns from tool errors are legitimate history.
            return groups;
        }

        function stage(snapshot, split, task = null) {
            const id = Branching.getCurrentPromptId();
            if (!id || id === "new_chat") throw new Error("保存会话后才能原地压缩");
            if (!snapshot?.trim()) throw new Error("摘要为空，无法压缩");
            // Previously generated memories are pinned. Never select
            // their own model replies for deletion in a later compression.
            // Archive classification is verified against the live DOM during
            // preflight. A serialized user index is not a safe deletion index
            // when previous compression exchanges are excluded from N rounds.
            const pinnedOlderRounds = [];
            const pinnedMemoryTexts = (split.older || [])
                .filter(turn => turn.role === "model" &&
                    CompactionTask.isPinnedSummary(String(turn.text || "")))
                .map(turn => String(turn.text).trim());
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

        function planManualCleanup(keepRounds = Number(userSettings.keepRounds) || 8) {
            const promptId = Branching.getCurrentPromptId();
            if (!promptId || promptId === "new_chat") {
                throw new Error("请先保存当前聊天");
            }
            if (!Number.isSafeInteger(keepRounds) || keepRounds < 1 || keepRounds > 100) {
                throw new Error("保留轮数必须是 1 到 100 的整数");
            }
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            const groups = collectGroups(turns);
            const regular = groups.filter(group =>
                group.kind !== "memory" && group.kind !== "opaque");
            // Historical compression requests and their replies never count
            // towards the user's recent N normal dialogue rounds. This holds
            // even if the model forgot to emit the memory marker.
            const archiveIndices = regular.flatMap((group, index) =>
                isArchiveGroup(group) ? [index] : []);
            const chatIndices = regular.flatMap((group, index) =>
                isArchiveGroup(group) ? [] : [index]);
            const hasOldArchiveThoughts = archiveIndices.some(index =>
                regular[index].turns.some(turn =>
                    normalizeType(turn) === "thought" && hasIndependentDeleteMenu(turn)));
            const hasTemporaryTasks = safeDisposablePrompts(turns).length > 0;
            if (chatIndices.length <= keepRounds &&
                !hasOldArchiveThoughts && !hasTemporaryTasks) {
                throw new Error("当前只有 " + chatIndices.length +
                    " 轮普通对话，没有超出保留范围的历史，也没有可独立清理的压缩思考块");
            }
            const olderGroupIndices = chatIndices.slice(
                0, Math.max(0, chatIndices.length - keepRounds));
            const pinnedOlderRounds = archiveIndices;
            const plan = {
                manual: true,
                promptId, snapshot: "", marker: null, memoryMarker: null,
                totalRounds: regular.length, olderRounds: olderGroupIndices.length,
                olderGroupIndices, recentRounds: keepRounds,
                pinnedOlderRounds, pinnedMemoryTexts: [], legacy: false,
                // UI may virtualize or reorder chat nodes; never delete a
                // stale selection after the page has changed.
                domIds: turns.map(turn => turn.id)
            };
            return plan;
        }

        async function manualCleanup() {
            if (deleting) return;
            // This path is independent of compression / task markers.
            // A user may clear old history without producing any summary.
            const plan = planManualCleanup();
            return commit(plan);
        }

        // All independently deletable Thinking blocks are eligible, including
        // those in recent ordinary rounds, summaries, tool/error messages.
        // No dialogue grouping is needed; unknown tool nodes remain opaque.
        function planThoughtCleanup() {
            const promptId = Branching.getCurrentPromptId();
            if (!promptId || promptId === "new_chat") {
                throw new Error("请先保存当前聊天");
            }
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            return {
                thoughtOnly: true, manual: true, promptId,
                domIds: turns.map(turn => turn.id)
            };
        }

        async function cleanupThoughts() {
            if (deleting) {
                PromptPanel.status("已有清理操作正在进行");
                return;
            }
            return commit(planThoughtCleanup());
        }

        async function verifyPersistedSummary(plan) {
            const capture = await fetchResolvedPromptThread(plan.promptId);
            if (!capture?.entries?.length) throw new Error("未能读取最新会话，停止删除");
            assertSupportedEntries(capture.entries, extractResolvedTurns(capture.entries));
            const turns = extractResolvedTurns(capture.entries);
            const users = turns.filter(turn => turn.role === "user");
            if (users.length < plan.totalRounds + 1) {
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

        function preflightDom(plan, savedInventory = null) {
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            // Thinking cleanup is independent of the dialogue structure:
            // tool calls, search results and failed replies are preserved.
            if (plan.thoughtOnly) {
                if (turns.length !== plan.domIds.length ||
                    turns.some((turn, index) =>
                        !turn.id || turn.id !== plan.domIds[index])) {
                    throw new Error("网页聊天节点在准备期间发生变化，请重新点击清理");
                }
                // The Google saved transcript and rendered UI expose
                // different structures. Never gate a native Thinking delete
                // on an exact archive-list comparison between them.
                const deletable = [];
                let skippedThoughts = 0;
                for (const turn of turns) {
                    if (normalizeType(turn) !== "thought") continue;
                    if (hasIndependentDeleteMenu(turn)) deletable.push(turn);
                    else skippedThoughts++;
                }
                const temporary = temporaryTasks(turns);
                const disposablePrompts = safeDisposablePrompts(turns);
                // Delete standalone Thinking and only truly orphaned
                // plugin USER requests. A paired MODEL is never targeted.
                deletable.push(...disposablePrompts);
                const targetIds = deletable.map(turn => turn.id);
                if (targetIds.some(id => !id) ||
                    new Set(targetIds).size !== targetIds.length) {
                    throw new Error("待清理思维链缺少稳定 ID，已停止");
                }
                const targets = new Set(targetIds);
                return {
                    thoughtOnly: true, targetIds,
                    protectedIds: turns.filter(turn => !targets.has(turn.id))
                        .map(turn => turn.id),
                    protectedSummaryCount: turns.filter(isProtectedTurn).length,
                    skippedThoughts,
                    skippedPrompts: temporary.length - disposablePrompts.length,
                    items: targetIds.length,
                    deletedOldRounds: 0,
                    deletedUserCount: disposablePrompts.length
                };
            }

            const groups = collectGroups(turns);
            // Protect recognized summaries by their native DOM nodes and
            // markers below. Cross-surface matching is diagnostic only:
            // Google can serialize a turn differently than it renders it.
            const regular = groups.filter(group =>
                group.kind !== "memory" && group.kind !== "opaque");
            const dialogueIndices = regular.flatMap((group, index) =>
                isArchiveGroup(group) ? [] : [index]);
            const actual = plan.manual ? regular.length : dialogueIndices.length;
            if (actual !== plan.totalRounds) {
                throw new Error("网页历史轮次已改变（检测到 " +
                    actual + "，预期 " + plan.totalRounds + "），请重新操作");
            }
            if (plan.manual && (turns.length !== plan.domIds.length ||
                turns.some((turn, index) => !turn.id || turn.id !== plan.domIds[index]))) {
                throw new Error("网页聊天节点在准备期间发生变化，请重新点击清理");
            }
            const getRole = turn => normalizeType(turn);
            const inferredArchiveIndices = regular.flatMap((group, index) =>
                isArchiveGroup(group) ? [index] : []);
            const pinned = new Set([
                ...(plan.pinnedOlderRounds || []),
                ...inferredArchiveIndices
            ]);
            for (const index of plan.pinnedOlderRounds || []) {
                if (index < 0 || index >= regular.length ||
                    !isArchiveGroup(regular[index])) {
                    throw new Error("历史压缩记忆与网页消息位置不一致，拒绝删除");
                }
            }
            if (plan.marker && plan.memoryMarker) {
                const latest = regular[regular.length - 1];
                if (!latest || !isArchiveRequest(latest.turns[0]) ||
                    !String(latest.turns[0].textContent || "").includes(plan.marker) ||
                    !latest.turns.some(turn => getRole(turn) === "model" &&
                        String(turn.textContent || "").includes(plan.memoryMarker))) {
                    throw new Error("最新摘要的永久记忆标识缺失，已拒绝任何删除");
                }
            }

            const targetTurns = [];
            const ignoredOldThoughts = new Set();
            const olderIndices = plan.manual ? plan.olderGroupIndices :
                dialogueIndices.slice(0, Math.max(0, dialogueIndices.length - plan.recentRounds));
            if (olderIndices.length !== plan.olderRounds) {
                throw new Error("普通对话与压缩记录的轮次边界不一致，已停止删除");
            }
            for (const i of olderIndices) {
                if (pinned.has(i)) continue;
                for (const turn of [...regular[i].turns].reverse()) {
                    if (getRole(turn) === "thought" && !turn.querySelector(
                        "ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"
                    )) {
                        // It may disappear with its parent response; avoid
                        // independently targeting an unclickable thought node.
                        ignoredOldThoughts.add(turn.id);
                    } else {
                        targetTurns.push(turn);
                    }
                }
            }

            // Archived MODEL replies are never deletion candidates. A
            // temporary USER request with an existing MODEL is also retained
            // until an independently verified, non-cascading way to remove
            // that request exists. Its Thinking can still be removed.
            let skippedThoughts = 0;
            for (const group of groups) {
                if (!isArchiveGroup(group) && group.kind !== "memory") continue;
                for (const turn of group.turns) {
                    if (getRole(turn) !== "thought") continue;
                    if (hasIndependentDeleteMenu(turn)) {
                        targetTurns.push(turn);
                    } else {
                        skippedThoughts++;
                    }
                }
            }

            const temporary = temporaryTasks(turns);
            const disposablePrompts = safeDisposablePrompts(turns);
            // Remove thinking first, then temporary auto-generated prompts.
            targetTurns.push(...disposablePrompts);
            const targetIds = targetTurns.map(turn => turn.id);
            if (targetIds.some(id => !id) || new Set(targetIds).size !== targetIds.length) {
                throw new Error("待删除消息缺少稳定 ID，拒绝执行");
            }
            const targetSet = new Set(targetIds);
            const protectedTurns = turns.filter(turn =>
                !targetSet.has(turn.id) && !ignoredOldThoughts.has(turn.id));
            if (protectedTurns.some(turn => !turn.id || targetSet.has(turn.id))) {
                throw new Error("保护内容与删除范围发生冲突");
            }
            if (targetTurns.some(turn => !turn.querySelector(
                "ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"
            ))) {
                throw new Error("待删除消息缺少独立的原生删除操作，拒绝执行");
            }
            const deletedUserCount = targetTurns.filter(turn => getRole(turn) === "user").length;
            const deletedOldRounds = olderIndices.filter(i => !pinned.has(i)).length;
            return {
                targetIds,
                protectedIds: protectedTurns.map(turn => turn.id),
                deletedUserCount, deletedOldRounds, pinnedCount: pinned.size,
                protectedSummaryCount: turns.filter(isProtectedTurn).length,
                skippedThoughts,
                skippedPrompts: temporary.length - disposablePrompts.length,
                groups: groups.length, items: targetIds.length
            };
        }

        async function findNativeDeleteItem(trigger) {
            // Angular's menu panels live in a global overlay. Selecting the
            // first visible "Delete" anywhere can delete an unrelated MODEL
            // turn. Only act on the panel controlled by this exact trigger.
            for (let attempt = 0; attempt < 12; attempt++) {
                const menuId = trigger?.getAttribute?.("aria-controls");
                const expanded = trigger?.getAttribute?.("aria-expanded");
                if (!menuId || expanded !== "true") {
                    await sleep(80);
                    continue;
                }
                const panel = document.getElementById(menuId);
                if (!panel || !panel.isConnected ||
                    panel.getAttribute?.("aria-hidden") === "true") {
                    await sleep(80);
                    continue;
                }
                const items = [...panel.querySelectorAll(
                    ".mat-mdc-menu-content button, [role=menuitem], button"
                )];
                const matched = items.filter(item => {
                    if (item.disabled ||
                        item.getAttribute?.("aria-disabled") === "true") return false;
                    const icon = item.querySelector?.(
                        "mat-icon, .material-symbols-outlined, [data-mat-icon-name]"
                    );
                    const label = [
                        item.getAttribute?.("aria-label"),
                        icon?.getAttribute?.("fonticon"),
                        icon?.getAttribute?.("data-mat-icon-name"),
                        icon?.textContent,
                        item.textContent
                    ].filter(Boolean).join(" ").toLowerCase();
                    return /(?:delete|trash|删除|移除)/i.test(label);
                });
                // Ambiguous or stale menus are unsafe for destructive clicks.
                if (matched.length === 1) return matched[0];
                if (matched.length > 1) return null;
                await sleep(80);
            }
            return null;
        }

        async function deleteOne(id, thoughtOnly = false) {
            const turn = document.getElementById(id);
            if (!turn || !turn.isConnected) {
                return { deleted: false, reason: "目标消息已不在页面" };
            }
            const type = normalizeType(turn);
            const isTask = Boolean(temporaryTaskId(turn));
            if (thoughtOnly && type !== "thought" && !isTask) {
                return { deleted: false, reason: "目标已变为其他消息类型" };
            }
            // Thinking still deletes, regardless of any marker in its text.
            if (type !== "thought" && isProtectedTurn(turn)) {
                return { deleted: false, reason: "消息包含 sujiu 保护标记" };
            }
            const liveTurns = [...document.querySelectorAll("ms-chat-turn")];
            const archiveIds = pairedArchiveIds(liveTurns);
            if (type === "model" && archiveIds.has(id)) {
                return { deleted: false, reason: "该模型回复属于插件压缩结果" };
            }
            if (isTask && temporaryTasks(liveTurns).some(task =>
                task.prompt.id === id && task.replies.length > 0)) {
                return { deleted: false,
                    reason: "原生删除该指令可能连带删除模型摘要，暂时保留" };
            }
            // Select the native trigger button, not its inner icon. Clicking
            // a stale icon can activate the wrong floating menu.
            const button = turn.querySelector("ms-chat-turn-options button.mat-mdc-menu-trigger") ||
                turn.querySelector("ms-chat-turn-options button");
            if (!button) {
                return { deleted: false, reason: "没有独立删除菜单" };
            }
            button.click();
            const item = await findNativeDeleteItem(button);
            if (!item) {
                return { deleted: false, reason: "原生菜单没有删除选项" };
            }
            item.focus?.();
            item.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, cancelable: true }));
            item.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true }));
            item.click();
            for (let i = 0; i < 100; i++) {
                if (!document.getElementById(id)) return { deleted: true };
                await sleep(100);
            }
            return { deleted: false, reason: "原生删除后消息尚未消失" };
        }

        async function commit(selectedPlan = null) {
            if (deleting) {
                PromptPanel.status("已有清理操作正在进行");
                return;
            }
            const plan = selectedPlan || pending;
            if (!plan) throw new Error("请先读取摘要或使用「直接清理旧历史」");
            // Acquire before *any* asynchronous preflight. All failures,
            // cancelled confirmations, and unknown tool records release it.
            deleting = true;
            state.branchInProgress = true;
            try {
            if (Branching.getCurrentPromptId() !== plan.promptId) {
                throw new Error("会话已切换，请重新读取摘要");
            }
            // Preflight still verifies the staged summary and protected
            // boundary; only post-deletion server polling is removed.
            if (!plan.manual) await verifyPersistedSummary(plan);
            // A cloud snapshot is useful for post-delete diagnostics, but
            // cannot be required for deletion: Google may render different
            // archive markers or omit tool/error entries in its saved form.
            // Native deletion targets are selected and protected in the DOM.
            let savedInventory = null;
            try {
                savedInventory = await readSavedArchiveInventory(
                    plan.promptId, Boolean(plan.thoughtOnly));
            } catch (error) {
                log.ui("Cloud archive baseline unavailable; continuing with " +
                    "native DOM protections", String(error?.message || error));
            }
            const preview = preflightDom(plan);
            const prompt = plan.thoughtOnly
                ? "清理全部思维链：准备删除 " + preview.items +
                    " 条思维链或素酒临时摘要指令。普通对话、联网搜索与工具记录、模型回复、" +
                    preview.protectedSummaryCount + " 条带保护标识的消息都会保留。" +
                    (preview.skippedThoughts ? "另有 " + preview.skippedThoughts +
                        " 个没有独立删除菜单的思考块将跳过。" : "") +
                    (preview.skippedPrompts ? "另有 " + preview.skippedPrompts +
                        " 条带模型回复或无删除菜单的临时输入会保留。" : "") +
                    "原生删除不可撤销，继续？"
                : (plan.manual ? "手动清理（无需摘要）：准备删除 " : "准备一次性清理 ") +
                preview.deletedOldRounds + " 轮旧历史" +
                "，保护 " + preview.protectedSummaryCount + " 条带保护标识的消息" +
                "，清理压缩思维链及没有模型回复的临时指令。" +
                (preview.skippedPrompts ? "另有 " + preview.skippedPrompts +
                    " 条带模型回复或无删除菜单的临时输入会保留。" : "") +
                "最近 " + plan.recentRounds + " 轮普通对话会保留。" +
                "无法识别的历史摘要仍需人工核对。" +
                (preview.skippedThoughts ? "另有 " + preview.skippedThoughts +
                    " 个无法独立删除的思考块会保留。" : "") +
                "删除操作使用 AI Studio 原生消息菜单，Google 会自行自动保存。全部删除后统一等待保存响应。该操作不可撤销，继续？";
            if (!preview.items) {
                PromptPanel.status(plan.thoughtOnly ?
                    "没有可独立删除的思维链或孤立临时指令；带模型摘要的临时输入安全保留。" :
                    "没有可删除的旧对话或独立思考块。带保护标识的消息均已保留。");
                return;
            }
            if (!confirm(prompt)) return;
            let processed = 0;
            const saveBaseline = SaveMonitor.attempts;
            let lastDeletionStartedAt = 0;
            try {
                const protectedSet = new Set(preview.protectedIds);
                let skipped = 0;
                for (const id of preview.targetIds) {
                    if (Branching.getCurrentPromptId() !== plan.promptId) {
                        throw new Error("清理过程中会话已经切换");
                    }
                    const attemptStartedAt = Date.now();
                    let result;
                    try {
                        result = await deleteOne(id, Boolean(plan.thoughtOnly));
                    } catch (error) {
                        result = { deleted: false,
                            reason: String(error?.message || error) };
                    }
                    if (result?.deleted) {
                        processed++;
                        lastDeletionStartedAt = attemptStartedAt;
                    } else {
                        skipped++;
                        log.ui("Skipping undeletable native turn", id,
                            result?.reason || "unknown");
                    }
                    // An Angular cascading delete may remove the summary
                    // alongside its temporary user prompt. Stop immediately
                    // if that happens, instead of deleting the rest blindly.
                    for (const protectedId of protectedSet) {
                        if (!document.getElementById(protectedId)) {
                            throw new Error("原生删除联动影响了受保护消息 " +
                                protectedId + "，已立即停止。请刷新并核对摘要");
                        }
                    }
                    PromptPanel.status("批量清理中：" + processed + " 条已删除，" +
                        skipped + " 条跳过，共 " + preview.items + " 条目标");
                }
                // The old per-turn path never invoked Save explicitly.
                // AI Studio's native delete action triggers UpdatePrompt
                // automatically. Observe one quiet period after the batch.
                if (!processed) {
                    PromptPanel.status("没有成功删除的消息，跳过 " + skipped +
                        " 项；所有模型摘要均已保留。");
                    return;
                }
                // Once the staged synthetic USER prompt has been removed,
                // its review plan is no longer actionable. Clear only that
                // stale plan; the MODEL archive remains in the conversation.
                if (pending?.marker && ![...document.querySelectorAll("ms-chat-turn")]
                    .some(turn => normalizeType(turn) === "user" &&
                        String(turn.textContent || "").includes(pending.marker))) {
                    pending = null;
                    PromptPanel.showCompactionReview(null);
                }
                PromptPanel.status("批量删除完成，等待 Google 原生自动保存请求结束……");
                const autosave = await SaveMonitor.waitForNativeSave(saveBaseline, lastDeletionStartedAt);
                if (!autosave.observed) {
                    PromptPanel.status("页面删除已完成，但未观察到 UpdatePrompt 自动保存。" +
                        "请确认 Google 页面已同步，保存前不要刷新。");
                    return;
                }
                if (!autosave.ok) {
                    PromptPanel.status("Google 原生自动保存返回 " + autosave.status +
                        "，请检查网络和页面状态，暂勿刷新。");
                    return;
                }
                // Re-read after the native save when available. Differences
                // in Google's serialization are reported, not used to lock
                // future cleanup or retroactively claim an undo is possible.
                let verificationNotice = "";
                if (savedInventory) {
                    try {
                        const afterInventory = await readSavedArchiveInventory(
                            plan.promptId, Boolean(plan.thoughtOnly));
                        try {
                            verifyArchiveInventory(savedInventory, afterInventory);
                            if (plan.thoughtOnly) {
                                verifyProtectedMessages(savedInventory, afterInventory);
                            }
                        } catch (error) {
                            verificationNotice = " 云端保存记录发生变化，请刷新核对：" +
                                String(error?.message || error);
                        }
                    } catch (error) {
                        verificationNotice = " 未能重新读取云端记录，请刷新核对。";
                    }
                }
                if (!selectedPlan) {
                    pending = null;
                    PromptPanel.showCompactionReview(null);
                }
                PromptPanel.status("批量删除已完成；观察到 " + autosave.attempts +
                    " 次原生 UpdatePrompt 响应，最后状态 " + autosave.status +
                    "。这表示保存请求有成功响应，刷新后可核对实际云端历史。" +
                    (preview.skippedThoughts ? "部分无法独立删除的思考块已保留。" : "") +
                    (skipped ? " 已跳过 " + skipped + " 条不可删除的目标。" : "") +
                    (preview.skippedPrompts ? " 另有 " + preview.skippedPrompts +
                        " 条临时输入未删除，以防连带清除模型摘要。" : "") +
                    verificationNotice);
            } catch (error) {
                PromptPanel.status("批量清理处理了 " + processed + "/" + preview.items +
                    " 个页面消息后停止：" + String(error?.message || error) +
                    "。请刷新检查服务器实际状态");
                throw error;
            } finally {
                deleting = false;
                state.branchInProgress = false;
            }
            } finally {
                // Outer guard also runs if cloud inspection or DOM preflight
                // fails before the user sees a confirmation dialog.
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

        return Object.freeze({
            stage, commit, manualCleanup, planManualCleanup,
            cleanupThoughts, planThoughtCleanup, cancel,
            collectGroups, preflightDom, archiveInventory,
            verifyArchiveInventory, verifyProtectedMessages,
            get pending() { return pending; }
        });
    })();
