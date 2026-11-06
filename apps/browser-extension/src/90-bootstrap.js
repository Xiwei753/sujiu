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
        runtimeInfo.panelInitialized = attempt("panel", () => PromptPanel.mount());
        attempt("events", installTopLevelHandlers);
        runtimeInfo.networkInitialized = attempt("network", () => NetworkMonitor.init());
        if (!env.isAppContext) attempt("toolbar", () => UI.init());
    }

    bootstrap();
