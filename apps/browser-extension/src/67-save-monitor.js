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
        // This does not create any write request. It observes the same
        // native UpdatePrompt completions that already followed message
        // deletion in v0.3.1 and earlier.
        async function waitForNativeSave(sinceAttempts, afterTime = 0, quietMs = 900, maxWaitMs = 10000) {
            const start = Date.now();
            let observed = false;
            let lastSeen = start;
            let lastAttempt = sinceAttempts;
            while (Date.now() - start < maxWaitMs) {
                if (attempts > lastAttempt) {
                    observed = true;
                    lastAttempt = attempts;
                    lastSeen = Date.now();
                }
                // The final deletion must have caused (or preceded) an
                // observed save completion; earlier saves in the batch alone
                // cannot establish that the last mutation was submitted.
                if (observed && Date.parse(last?.time || "") < afterTime) {
                    observed = false;
                }
                if (observed && Date.now() - lastSeen >= quietMs) {
                    return { observed: true, ok: Boolean(last?.ok),
                        status: last?.status || 0, attempts: attempts - sinceAttempts };
                }
                await new Promise(resolve => setTimeout(resolve, 100));
            }
            observed = observed && Date.parse(last?.time || "") >= afterTime;
            return { observed, ok: observed && Boolean(last?.ok),
                status: observed ? last?.status || 0 : 0,
                attempts: Math.max(0, attempts - sinceAttempts) };
        }
        return Object.freeze({
            record,
            waitForNativeSave,
            get latest() { return last; },
            get attempts() { return attempts; }
        });
    })();
