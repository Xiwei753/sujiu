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

        // Fail closed when an older archive has lost its exact memory marker.
        // Older plugin versions and model responses may not have a working
        // [[SUJIU_MEMORY_V1:...]] label, but the reply is still user history.
        function isArchiveReply(turn) {
            if (normalizeType(turn) !== "model") return false;
            const text = String(turn.textContent || "");
            if (CompactionTask.isPinnedSummary(text)) return true;
            // Prior versions emitted XML-wrapped snapshots without a memory
            // marker. These replies must remain intact on subsequent runs.
            if (/(?:<|&lt;)\/?state_snapshot(?:>|&gt;)/i.test(text)) return true;
            const headings = [
                "【历史范围与时间锚点】", "【稳定设定与约束】",
                "【人物与关系】", "【关键事件时间线】",
                "【重要状态变化】", "【本段历史截点】",
                "【连续性校验】", "【未解决线索与悬而未决的信息】",
                "【稳定设定】", "【已发生的关键事件】",
                "【截至压缩截点的状态】", "【时间线衔接】"
            ];
            const count = headings.filter(heading => text.includes(heading)).length;
            return count >= 2 || /^(?:历史摘要|长期记忆|压缩记忆|剧情摘要|history summary|memory summary|conversation summary|story summary|context summary)\s*[:：]/i.test(text.trim());
        }

        function isArchiveRequest(turn) {
            if (normalizeType(turn) !== "user") return false;
            const text = String(turn.textContent || "");
            const excerpt = text.slice(0, 800);
            return /\[\[SUJIU_SCOPE_V2:/i.test(text) ||
                // Legacy and user-authored summary instructions can lack a marker.
                /(?:<|&lt;)state_snapshot(?:>|&gt;)/i.test(text) ||
                text.includes("Summarize the earlier role-play conversation") ||
                text.includes("请为这段长期对话制作按剧情时间线整理的精炼记忆") ||
                text.includes("请只根据本次请求中实际提供的历史记录") ||
                text.includes("你是负责长期会话连续性的记忆整理员") ||
                (text.includes("历史范围与时间锚点") &&
                 text.includes("关键事件时间线")) ||
                (/(?:摘要|总结|梳理|压缩|归纳|概括)/.test(excerpt) &&
                 /(?:前文|历史|剧情|对话|聊天|情节|记忆|人物关系|时间线|之前发生|已有内容)/.test(excerpt)) ||
                (/(?:summari[sz]e|summary|recap|condense|compress(?:ion)?|memory snapshot)/i.test(excerpt) &&
                 /(?:earlier|previous|prior|conversation|role.?play|history|chat|story|dialogue|context)/i.test(excerpt));
        }

        function isArchiveGroup(group) {
            return group?.kind === "round" &&
                (isArchiveRequest(group.turns[0]) ||
                 group.turns.some(isArchiveReply));
        }

        // Stable memory markers live in Google-saved message text. We cannot
        // assume AI Studio persists custom private message metadata. Build a
        // full archive inventory from the saved transcript, not just the
        // newest compression task kept in localStorage.
        function archiveInventory(turns, roleOf, textOf) {
            const markers = new Map();
            let legacyReplies = 0;
            let compressionRequests = 0;
            for (const turn of turns) {
                const role = roleOf(turn);
                const text = String(textOf(turn) || "");
                if (role === "user") {
                    if (RoundSplitter.isCompressionPrompt({ role, text })) {
                        compressionRequests++;
                    }
                } else if (role === "model") {
                    const ids = new Set(CompactionTask.getMemoryMarkers(text)
                        .map(marker => marker.toLowerCase()));
                    for (const marker of ids) {
                        markers.set(marker, (markers.get(marker) || 0) + 1);
                    }
                    if (!ids.size && RoundSplitter.isMemory({ role, text })) {
                        legacyReplies++;
                    }
                }
            }
            return { markers, legacyReplies, compressionRequests };
        }

        function verifyArchiveInventory(expected, actual) {
            const allMarkersMatch = expected.markers.size === actual.markers.size &&
                [...expected.markers].every(([marker, count]) =>
                    actual.markers.get(marker) === count);
            if (!allMarkersMatch ||
                expected.legacyReplies !== actual.legacyReplies ||
                expected.compressionRequests !== actual.compressionRequests) {
                throw new Error("Google 云端与网页的历史压缩摘要不一致，已停止删除以保护所有旧摘要");
            }
        }

        async function readSavedArchiveInventory(promptId) {
            const capture = await fetchResolvedPromptThread(promptId);
            if (!capture?.entries?.length) {
                throw new Error("无法读取 Google 云端完整历史，拒绝清理");
            }
            const turns = extractResolvedTurns(capture.entries);
            assertSupportedEntries(capture.entries, turns);
            const inventory = archiveInventory(turns,
                turn => turn.role === "model" && isThoughtOnlyTurn(turn) ?
                    "thought" : turn.role,
                turn => turn.text);
            // A thought-only cleanup must not silently lose a normal reply
            // that happens to lack a Sujiu-specific memory marker.
            inventory.protectedMessages = turns
                .filter(turn => !isThoughtOnlyTurn(turn))
                .map(turn => [turn.role, String(turn.text || "")]);
            return inventory;
        }

        function verifyProtectedMessages(before, after) {
            if (JSON.stringify(before.protectedMessages) !==
                JSON.stringify(after.protectedMessages)) {
                throw new Error("Google 云端普通对话或摘要发生变化，已停止；请检查历史记录");
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
                if (type === "unknown") throw new Error("发现未识别的消息类型，已停止删除");
                if (type === "user") {
                    groups.push({ turns: [turn], hasModel: false, kind: "round" });
                } else {
                    const group = groups[groups.length - 1];
                    const hasMemory = type === "model" && isArchiveReply(turn);
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
            const regular = groups.filter(group => group.kind !== "memory");
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
            if (chatIndices.length <= keepRounds && !hasOldArchiveThoughts) {
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

        // Independent cleanup leaves all user messages, model replies and
        // compaction archives in place. Only thought turns preceding the
        // configured protected recent dialogue window may be targeted, plus
        // independently deletable thought turns in archive exchanges.
        function planThoughtCleanup(keepRounds = Number(userSettings.keepRounds) || 8) {
            const promptId = Branching.getCurrentPromptId();
            if (!promptId || promptId === "new_chat") {
                throw new Error("请先保存当前聊天");
            }
            if (!Number.isSafeInteger(keepRounds) || keepRounds < 1 || keepRounds > 100) {
                throw new Error("保留轮数必须是 1 到 100 的整数");
            }
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            const groups = collectGroups(turns);
            const regular = groups.filter(group => group.kind !== "memory");
            const dialogueIndices = regular.flatMap((group, index) =>
                isArchiveGroup(group) ? [] : [index]);
            const olderGroupIndices = dialogueIndices.slice(
                0, Math.max(0, dialogueIndices.length - keepRounds));
            return {
                thoughtOnly: true, manual: true, promptId,
                totalRounds: regular.length,
                olderRounds: olderGroupIndices.length, olderGroupIndices,
                recentRounds: keepRounds, pinnedOlderRounds: [],
                domIds: turns.map(turn => turn.id)
            };
        }

        async function cleanupThoughts() {
            if (deleting) return;
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
            const groups = collectGroups(turns);
            if (savedInventory) {
                const visibleInventory = archiveInventory(turns,
                    normalizeType, turn => turn.textContent);
                verifyArchiveInventory(savedInventory, visibleInventory);
            }
            const regular = groups.filter(group => group.kind !== "memory");
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
            // If an old prompt has already been removed, its summary becomes
            // a standalone MODEL message. The immediately preceding exchange
            // might still be linked to it by Google's native delete action.
            // Preserve that neighbor rather than risk a cascading deletion.
            for (let index = 1; index < groups.length; index++) {
                if (groups[index].kind !== "memory") continue;
                const previous = groups[index - 1];
                const previousIndex = regular.indexOf(previous);
                if (previousIndex >= 0) pinned.add(previousIndex);
            }
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

            if (plan.thoughtOnly) {
                const archiveGroups = groups.filter(group =>
                    group.kind === "memory" || isArchiveGroup(group));
                const allowed = new Set([
                    ...archiveGroups,
                    ...(plan.olderGroupIndices || []).map(index => regular[index])
                ]);
                const targetTurns = [];
                let skippedThoughts = 0;
                for (const group of groups) {
                    if (!allowed.has(group)) continue;
                    for (const turn of group.turns) {
                        if (getRole(turn) !== "thought") continue;
                        if (hasIndependentDeleteMenu(turn)) targetTurns.push(turn);
                        else skippedThoughts++;
                    }
                }
                const targetIds = targetTurns.map(turn => turn.id);
                if (targetIds.some(id => !id) ||
                    new Set(targetIds).size !== targetIds.length) {
                    throw new Error("待清理的思维链缺少稳定 ID，已停止");
                }
                const targetSet = new Set(targetIds);
                const protectedTurns = turns.filter(turn => !targetSet.has(turn.id));
                if (protectedTurns.some(turn => !turn.id) ||
                    targetTurns.some(turn => getRole(turn) !== "thought")) {
                    throw new Error("思维链与受保护消息存在冲突，已停止");
                }
                return {
                    thoughtOnly: true, targetIds,
                    protectedIds: protectedTurns.map(turn => turn.id),
                    deletedUserCount: 0, deletedOldRounds: 0,
                    protectedSummaryCount: inferredArchiveIndices.length +
                        groups.filter(group => group.kind === "memory").length,
                    skippedThoughts, groups: groups.length, items: targetIds.length
                };
            }

            // The visible summary is more important than cleaning its prompt.
            // Native AI Studio deletion of a USER turn may cascade to the
            // paired MODEL reply. Therefore never auto-delete a summary's
            // user prompt until independent deletion semantics are verified.
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

            // All archive replies are protected, including old ones without
            // markers and the current summary. Only independently deletable
            // thought blocks are eligible for cleanup. Never delete the
            // compression user prompt: some Google versions remove the reply
            // along with it, irreversibly destroying the archive.
            let skippedThoughts = 0;
            for (const group of groups) {
                if (!isArchiveGroup(group)) continue;
                if (!group.turns.some(turn => getRole(turn) === "model")) {
                    throw new Error("检测到缺少可见回复的历史压缩记录，停止清理");
                }
                for (const turn of group.turns) {
                    if (getRole(turn) !== "thought") continue;
                    if (hasIndependentDeleteMenu(turn)) {
                        targetTurns.push(turn);
                    } else {
                        skippedThoughts++;
                    }
                }
            }

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
                protectedSummaryCount: inferredArchiveIndices.length +
                    groups.filter(group => group.kind === "memory").length,
                skippedThoughts, groups: groups.length, items: targetIds.length
            };
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

        async function deleteOne(id, thoughtOnly = false) {
            const turn = document.getElementById(id);
            if (!turn || !turn.isConnected) return;
            // Inspect the live turn immediately before opening Google's
            // destructive native menu. A stale ID must never target a reply.
            if (thoughtOnly && normalizeType(turn) !== "thought") {
                throw new Error("待删除的思维链已经变成其他消息类型，已停止");
            }
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

        async function commit(selectedPlan = null) {
            if (deleting) return;
            const plan = selectedPlan || pending;
            if (!plan) throw new Error("请先读取摘要或使用「直接清理旧历史」");
            if (Branching.getCurrentPromptId() !== plan.promptId) {
                throw new Error("会话已切换，请重新读取摘要");
            }
            // Preflight still verifies the staged summary and protected
            // boundary; only post-deletion server polling is removed.
            if (!plan.manual) await verifyPersistedSummary(plan);
            // Both summary-based cleanup and "direct" manual cleanup must
            // cross-check every saved memory. A missing archived turn can be
            // invisible to ordinary round-count checks.
            const savedInventory = await readSavedArchiveInventory(plan.promptId);
            const preview = preflightDom(plan, savedInventory);
            const prompt = plan.thoughtOnly
                ? "独立清理旧思维链：准备删除 " + preview.items +
                    " 个独立思维链消息。全部普通对话、模型回复、" +
                    preview.protectedSummaryCount + " 条压缩摘要与压缩指令都会保留。" +
                    "最近 " + plan.recentRounds + " 轮普通对话附带的思维链也会保留。" +
                    (preview.skippedThoughts ? "另有 " + preview.skippedThoughts +
                        " 个没有独立删除菜单的思考块将跳过。" : "") +
                    "会核对 Google 云端保存结果；原生删除不可撤销，继续？"
                : (plan.manual ? "手动清理（无需摘要）：准备删除 " : "准备一次性清理 ") +
                preview.deletedOldRounds + " 轮旧历史" +
                "，保护 " + preview.protectedSummaryCount + " 条已识别的压缩摘要" +
                "，清理可独立删除的压缩思维链。" +
                "为避免 Google 联动删除摘要，压缩指令暂时保留。" +
                "最近 " + plan.recentRounds + " 轮普通对话会保留。" +
                "无法识别的历史摘要仍需人工核对。" +
                (preview.skippedThoughts ? "另有 " + preview.skippedThoughts +
                    " 个无法独立删除的思考块会保留。" : "") +
                "删除操作使用 AI Studio 原生消息菜单，Google 会自行自动保存。全部删除后统一等待保存响应。该操作不可撤销，继续？";
            if (!preview.items) {
                PromptPanel.status(plan.thoughtOnly ?
                    "没有可独立删除的旧思维链。所有普通消息和压缩摘要保持原样。" :
                    "没有可安全删除的旧对话或独立思考块。压缩摘要和对应指令均已保护。");
                return;
            }
            if (!confirm(prompt)) return;
            deleting = true;
            state.branchInProgress = true;
            let processed = 0;
            const saveBaseline = SaveMonitor.attempts;
            let lastDeletionStartedAt = 0;
            try {
                const protectedSet = new Set(preview.protectedIds);
                for (const id of preview.targetIds) {
                    if (Branching.getCurrentPromptId() !== plan.promptId) {
                        throw new Error("清理过程中会话已经切换");
                    }
                    lastDeletionStartedAt = Date.now();
                    await deleteOne(id, Boolean(plan.thoughtOnly));
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
                        "，原生页面将自行处理自动保存");
                }
                // The old per-turn path never invoked Save explicitly.
                // AI Studio's native delete action triggers UpdatePrompt
                // automatically. Observe one quiet period after the batch.
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
                // A successful UpdatePrompt response alone does not prove
                // that Google retained all historical memory records.
                // Re-read the saved conversation after the native autosave.
                const afterInventory = await readSavedArchiveInventory(plan.promptId);
                verifyArchiveInventory(savedInventory, afterInventory);
                if (plan.thoughtOnly) {
                    verifyProtectedMessages(savedInventory, afterInventory);
                }
                if (!selectedPlan) {
                    pending = null;
                    PromptPanel.showCompactionReview(null);
                }
                PromptPanel.status("批量删除已完成；观察到 " + autosave.attempts +
                    " 次原生 UpdatePrompt 响应，最后状态 " + autosave.status +
                    "。这表示保存请求有成功响应，刷新后可核对实际云端历史。" +
                    (preview.skippedThoughts ? "部分无法独立删除的思考块已保留。" : ""));
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

        return Object.freeze({
            stage, commit, manualCleanup, planManualCleanup,
            cleanupThoughts, planThoughtCleanup, cancel,
            collectGroups, preflightDom, archiveInventory,
            verifyArchiveInventory, verifyProtectedMessages,
            get pending() { return pending; }
        });
    })();
