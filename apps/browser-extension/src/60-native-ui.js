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
            log.ui("Toolbar not found, wait for next mutation");
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
