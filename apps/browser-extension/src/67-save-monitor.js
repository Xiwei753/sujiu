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
