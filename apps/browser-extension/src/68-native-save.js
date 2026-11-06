    // Use AI Studio's own explicit Save control once after all native
    // deletions. Do not synthesize a private UpdatePrompt request: a 200
    // response does not prove that the current chat data was persisted.
    const NativeSave = (() => {
        const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
        const labelsOf = element => [
            element.getAttribute?.("aria-label"),
            element.getAttribute?.("title"),
            element.getAttribute?.("data-tooltip"),
            element.textContent,
            element.querySelector?.("mat-icon, .material-symbols-outlined")?.textContent
        ].filter(Boolean).map(value => String(value).trim().toLowerCase());
        const SAVE = /^(save|save prompt|保存|保存提示词|保存提示|儲存|儲存提示詞|save changes)$/i;
        const BLOCK = /save as|save a copy|duplicate|另存|保存副本|复制|export|导出|save to library/i;

        function isSaveControl(element) {
            if (!element || element.disabled ||
                element.getAttribute?.("aria-disabled") === "true") return false;
            const labels = labelsOf(element);
            if (labels.some(value => BLOCK.test(value))) return false;
            return labels.some(value => SAVE.test(value));
        }
        function findDirect() {
            const containers = [
                document.querySelector("ms-toolbar"),
                document.querySelector("ms-playground-toolbar"),
                document.querySelector("header")
            ].filter(Boolean);
            for (const container of containers) {
                const buttons = container.querySelectorAll("button, [role=button]");
                const match = [...buttons].find(isSaveControl);
                if (match) return match;
            }
            return null;
        }
        function findMenu() {
            const buttons = [...document.querySelectorAll(
                ".cdk-overlay-container [role=menuitem], .cdk-overlay-container .mat-mdc-menu-content button"
            )];
            return buttons.find(isSaveControl) || null;
        }
        function findMore() {
            const toolbar = document.querySelector("ms-toolbar, ms-playground-toolbar, header");
            if (!toolbar) return null;
            return [...toolbar.querySelectorAll("button, [role=button]")].find(button => {
                const label = labelsOf(button).join(" ");
                return /\bmore (options|actions)\b|更多选项|更多操作|more_vert/.test(label) &&
                    !button.disabled;
            }) || null;
        }
        async function triggerOnce() {
            // Distinguish a native save action from an unrelated "Save As",
            // browser Save Page, export, or another plugin's controls.
            const direct = findDirect();
            if (direct) {
                direct.click();
                return { triggered: true, via: "toolbar" };
            }
            let item = findMenu();
            if (item) {
                item.click();
                return { triggered: true, via: "menu" };
            }
            const more = findMore();
            if (!more) return { triggered: false, reason: "找不到当前 AI Studio 的原生保存按钮或更多菜单" };
            more.click();
            for (let i = 0; i < 10; i++) {
                await sleep(100);
                item = findMenu();
                if (item) {
                    item.click();
                    return { triggered: true, via: "menu" };
                }
            }
            // Close only the menu we opened. No second save attempt.
            document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
            return { triggered: false, reason: "更多菜单中没有可确认的原生「保存」操作" };
        }
        return Object.freeze({ triggerOnce, isSaveControl });
    })();
