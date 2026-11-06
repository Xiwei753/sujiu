    const InPlaceCompaction = (() => {
        let pending = null;
        let deleting = false;
        const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

        function normalizeType(turn) {
            const raw = (turn.getAttribute("data-turn-type") ||
                turn.getAttribute("data-turn-role") ||
                turn.dataset?.turnType || turn.dataset?.turnRole || "").toLowerCase();
            if (raw === "user" || raw === "prompt") return "user";
            if (raw === "thought") return "thought";
            if (raw === "model" || raw === "text" || raw === "response") return "model";
            return "unknown";
        }

        // Use every rendered native turn, including thought blocks. An
        // incomplete / virtualized DOM is never safe for destructive changes.
        function collectGroups(turns) {
            const groups = [];
            for (const turn of turns) {
                const type = normalizeType(turn);
                if (type === "unknown") throw new Error("发现未识别的消息类型，已停止删除");
                if (type === "user") groups.push({ turns: [turn], hasModel: false });
                else {
                    const group = groups[groups.length - 1];
                    if (!group) throw new Error("历史消息的顺序异常");
                    group.turns.push(turn);
                    if (type === "model") group.hasModel = true;
                }
            }
            if (groups.some(group => !group.hasModel)) throw new Error("发现未完成的轮次，已停止删除");
            return groups;
        }

        function stage(snapshot, split) {
            const id = Branching.getCurrentPromptId();
            if (!id || id === "new_chat") throw new Error("保存会话后才能原地压缩");
            if (!snapshot?.trim()) throw new Error("摘要为空，无法压缩");
            pending = {
                promptId: id,
                snapshot: snapshot.trim(),
                olderRounds: split.olderRounds,
                recentRounds: split.recentRounds,
                totalRounds: split.totalRounds,
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
            const last = turns[turns.length - 1];
            if (last?.role !== "model" || !last.text.includes(plan.snapshot.slice(0, 70))) {
                throw new Error("摘要尚未保存在 Google 会话中，拒绝删除旧消息");
            }
            return { capture, turns };
        }

        function preflightDom(plan) {
            const turns = [...document.querySelectorAll("ms-chat-turn")];
            const groups = collectGroups(turns);
            if (groups.length !== plan.totalRounds + 1) {
                throw new Error("网页未完整加载所有历史轮次（检测到 " +
                    groups.length + "，预期 " + (plan.totalRounds + 1) + "），拒绝删除");
            }
            const selected = groups.slice(0, plan.olderRounds).flatMap(group => group.turns);
            const protectedTurns = groups.slice(plan.olderRounds).flatMap(group => group.turns);
            const ids = selected.map(turn => turn.id);
            if (ids.some(id => !id) || new Set(ids).size !== ids.length) {
                throw new Error("部分旧消息没有稳定 ID，无法安全删除");
            }
            if (protectedTurns.some(turn => ids.includes(turn.id))) {
                throw new Error("删除范围与保留范围重叠");
            }
            if (selected.some(turn => !turn.querySelector("ms-chat-turn-options button, ms-chat-turn-options span.ms-button-icon-symbol"))) {
                throw new Error("部分消息不支持原生删除菜单");
            }
            return { targetIds: ids, groups: groups.length, items: selected.length };
        }

        async function findNativeDeleteItem() {
            for (let attempt = 0; attempt < 12; attempt++) {
                const items = [...document.querySelectorAll(
                    ".cdk-overlay-container .mat-mdc-menu-content button, .cdk-overlay-container [role=menuitem]"
                )];
                const target = items.find(item => {
                    const icon = item.querySelector("mat-icon, .material-symbols-outlined");
                    const label = (item.getAttribute("aria-label") || "") + " " +
                        (icon?.textContent || "") + " " + (item.textContent || "");
                    return /(?:^|\s)(delete|删除)(?:\s|$)/i.test(label.trim());
                });
                if (target) return target;
                await sleep(80);
            }
            return null;
        }

        async function deleteOne(id) {
            const turn = document.getElementById(id);
            if (!turn || !turn.isConnected) return;
            const button = turn.querySelector("ms-chat-turn-options button.mat-mdc-menu-trigger") ||
                turn.querySelector("ms-chat-turn-options button") ||
                turn.querySelector("ms-chat-turn-options span.ms-button-icon-symbol");
            if (!button) throw new Error("消息 " + id + " 缺少原生操作菜单");
            button.click();
            const item = await findNativeDeleteItem();
            if (!item) throw new Error("找不到 Google 原生删除选项，已停止");
            item.click();
            // A further confirmation dialog is deliberately not bypassed.
            for (let i = 0; i < 30; i++) {
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
                throw new Error("会话已切换，需重新压缩");
            }
            await verifyPersistedSummary(plan);
            const preview = preflightDom(plan);
            if (!confirm("即将在当前聊天中删除最旧的 " + plan.olderRounds +
                    " 轮（包括关联思考内容），保留最近 " + plan.recentRounds +
                    " 轮和刚生成的摘要。此操作无法撤销，确认继续？")) return;
            deleting = true;
            state.branchInProgress = true;
            let processed = 0;
            try {
                for (const id of preview.targetIds) {
                    if (Branching.getCurrentPromptId() !== plan.promptId) {
                        throw new Error("删除过程中会话发生切换");
                    }
                    await deleteOne(id);
                    processed++;
                    PromptPanel.status("正在原地清理：" + processed + "/" + preview.items);
                }
                const after = await fetchResolvedPromptThread(plan.promptId);
                if (!after?.entries?.length) throw new Error("无法验证 Google 端保存结果");
                const remaining = extractResolvedTurns(after.entries);
                const remainingUsers = remaining.filter(t => t.role === "user").length;
                if (remainingUsers !== plan.recentRounds + 1) {
                    throw new Error("已执行删除，但云端仍有 " + remainingUsers +
                        " 条用户消息；请刷新确认，避免重复操作");
                }
                pending = null;
                PromptPanel.showCompactionReview(null);
                PromptPanel.status("原地压缩完成，聊天地址保持不变。建议刷新后核对历史。");
            } catch (error) {
                PromptPanel.status("已处理 " + processed + "/" + preview.items + " 条后停止：" +
                    String(error?.message || error) + "。请刷新检查现有历史。");
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
