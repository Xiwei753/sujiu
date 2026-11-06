    function bootstrap() {
        const runtimeInfo = {
            version: SCRIPT_VERSION,
            href: location.href,
            loadedAt: new Date().toISOString()
        };
        env.topWindow.__AISTUDIO_COMPRESSOR_RUNTIME__ = runtimeInfo;
        console.info(Config.TAGS.UI, "Script version", runtimeInfo.version, "loaded", runtimeInfo);

        installTopLevelHandlers();
        // All configuration is local to the embedded Sujiu panel.
        NetworkMonitor.init();
        if (!env.isAppContext) {
            UI.init();
            PromptPanel.mount();
        }
    }

    bootstrap();
