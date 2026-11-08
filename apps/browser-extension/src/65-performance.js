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
