    const NetworkMonitor = {
        init() {
            const label = env.isAppContext ? "app-window" : "top-window";
            hookContext(window, label);

            window.addEventListener("DOMContentLoaded", () => {
                hookContext(window, `${label} DOMContentLoaded`);
                hookChildFrames("DOMContentLoaded");
            });

            window.addEventListener("load", () => {
                hookContext(window, `${label} load`);
                hookChildFrames("load");
            });

            setInterval(() => hookChildFrames("poll"), 2000);
        }
    };

    function hookChildFrames(reason) {
        if (!document?.querySelectorAll) return;
        for (const frame of document.querySelectorAll("iframe")) {
            const ctx = frame.contentWindow;
            if (!ctx || !canAccessWindow(ctx)) continue;
            try {
                hookContext(ctx, `iframe ${frame.src || frame.name || "unnamed"} via ${reason}`);
            } catch (err) {
                log.net("cannot hook iframe:", err);
            }
        }
    }

    function hookContext(ctx, label) {
        if (!canAccessWindow(ctx)) {
            log.net(`skip hooking for ${label}: cross-origin window`);
            return;
        }
        installFetchHook(ctx, label);
        installXHRHook(ctx, label);
    }

    function canAccessWindow(ctx) {
        try {
            return ctx && ctx.location && ctx.location.origin === location.origin;
        } catch {
            return false;
        }
    }

    function installFetchHook(ctx, label) {
        if (!ctx || typeof ctx.fetch !== "function") {
            log.net(`skip hooking for ${label}: fetch not available`);
            return;
        }
        if (ctx.__aistudio_compressor_fetch_hooked) return;

        const originalFetch = ctx.fetch.bind(ctx);

        ctx.fetch = async function interceptFetch(input, init = {}) {
            const url = typeof input === "string" ? input : input?.url;
            const shouldLogGenerate = isTargetRequest(url, "GENERATE");
            const shouldCaptureResolve = isTargetRequest(url, "RESOLVE");
            if (!shouldLogGenerate && !shouldCaptureResolve) {
                return originalFetch(input, init);
            }
            let request;
            try {
                request = new Request(input, init);
            } catch {
                // Never break the host application because our inspector cannot clone a request.
                return originalFetch(input, init);
            }
            if (shouldLogGenerate) await logRequestPayload(request);
            const response = await originalFetch(input, init);

            if (shouldLogGenerate || shouldCaptureResolve) {
                void logResponsePayload(request.url, response.clone());
            }

            return response;
        };

        ctx.__aistudio_compressor_fetch_hooked = true;
        log.net(`fetch hooked in ${label}`);

        if (ctx !== unsafeWindow && typeof unsafeWindow.fetch === "function" && !unsafeWindow.__aistudio_compressor_fetch_hooked) {
            unsafeWindow.fetch = ctx.fetch;
            unsafeWindow.__aistudio_compressor_fetch_hooked = true;
            log.net("fetch mirrored to unsafeWindow");
        }
    }

    async function logRequestPayload(request) {
        try {
            const clonedRequest = request.clone();
            const bodyText = await clonedRequest.text();
            const headers = Object.fromEntries(clonedRequest.headers.entries());
            rememberGoogleApiKey(headers, clonedRequest.url);

            // Sensitive headers are never logged.
            try {

            } catch {

            }

            emitNetworkEvent(Config.EVENTS.REQUEST, {
                url: clonedRequest.url,
                headers,
                bodyText
            });
        } catch (err) {
            console.warn(Config.TAGS.NET, "Failed to read request body:", err);
        }
    }

    async function logResponsePayload(url, response) {
        try {
            const bodyText = await response.text();
            const parsed = safeParseJSON(bodyText);
            maybeCaptureResolvedPromptThread(url, bodyText, parsed);


            if (isTargetRequest(url, "RESOLVE")) {

            } else if (parsed) {

            } else {

            }

            emitNetworkEvent(Config.EVENTS.RESPONSE, {
                url,
                status: response.status,
                statusText: response.statusText,
                bodyText
            });
        } catch (err) {
            console.warn(Config.TAGS.NET, "Failed to read response body:", err);
        }
    }

    function installXHRHook(ctx, label) {
        if (!ctx || typeof ctx.XMLHttpRequest !== "function") {
            log.net(`skip hooking XHR for ${label}: XMLHttpRequest not available`);
            return;
        }
        if (ctx.__aistudio_compressor_xhr_hooked) return;

        const OriginalXHR = ctx.XMLHttpRequest;

        function WrappedXHR() {
            const realXHR = new OriginalXHR();
            let monitored = false;
            let requestUrl = null;
            const requestHeaders = {};

            const originalOpen = realXHR.open;
            realXHR.open = function (...args) {
                const [, url] = args;
                requestUrl = url || null;
                monitored = Boolean(url) && (isTargetRequest(url, "GENERATE") || isTargetRequest(url, "RESOLVE"));
                if (monitored) {
                    log.net("Monitored XHR detected in", label, url);
                }
                return originalOpen.apply(this, args);
            };

            const originalSetRequestHeader = realXHR.setRequestHeader;
            realXHR.setRequestHeader = function (name, value) {
                if (typeof name === "string") {
                    requestHeaders[name] = value;
                    rememberGoogleApiKey(requestHeaders, resolveUrl(requestUrl));
                }
                return originalSetRequestHeader.apply(this, arguments);
            };

            const originalSend = realXHR.send;
            realXHR.send = function (body) {
                if (monitored) {
                    const absoluteUrl = resolveUrl(requestUrl);
                    const bodyText = bodyToText(body);
                    const headers = Object.keys(requestHeaders).length ? { ...requestHeaders } : null;
                    rememberGoogleApiKey(headers, absoluteUrl);


                    if (bodyText) {
                        try {

                        } catch {

                        }
                    } else {

                    }

                    emitNetworkEvent(Config.EVENTS.REQUEST, {
                        url: absoluteUrl,
                        headers,
                        bodyText
                    });
                }
                return originalSend.apply(this, arguments);
            };

            realXHR.addEventListener("readystatechange", function () {
                if (realXHR.readyState !== 4 || !monitored) {
                    return;
                }
                const responseUrl = realXHR.responseURL || resolveUrl(requestUrl);
                const parsed = safeParseJSON(realXHR.responseText);
                maybeCaptureResolvedPromptThread(responseUrl, realXHR.responseText, parsed);


                if (isTargetRequest(responseUrl, "RESOLVE")) {

                } else if (parsed) {

                } else {

                }

                emitNetworkEvent(Config.EVENTS.RESPONSE, {
                    url: responseUrl,
                    status: realXHR.status,
                    statusText: realXHR.statusText,
                    bodyText: realXHR.responseText
                });
            });

            return realXHR;
        }

        ctx.XMLHttpRequest = WrappedXHR;
        ctx.__aistudio_compressor_xhr_hooked = true;
        log.net(`XMLHttpRequest hooked in ${label}`);

        if (ctx !== unsafeWindow && typeof unsafeWindow.XMLHttpRequest === "function" && !unsafeWindow.__aistudio_compressor_xhr_hooked) {
            unsafeWindow.XMLHttpRequest = WrappedXHR;
            unsafeWindow.__aistudio_compressor_xhr_hooked = true;
            log.net("XMLHttpRequest mirrored to unsafeWindow");
        }
    }

    function resolveUrl(url) {
        if (!url) return "";
        try {
            return new URL(url, location.origin).toString();
        } catch {
            return url;
        }
    }

    function bodyToText(body) {
        if (!body) return null;
        if (typeof body === "string") return body;
        if (body instanceof ArrayBuffer) {
            try {
                return new TextDecoder().decode(body);
            } catch {
                return null;
            }
        }
        if (ArrayBuffer.isView(body)) {
            try {
                return new TextDecoder().decode(body.buffer);
            } catch {
                return null;
            }
        }
        return null;
    }

    function isTargetRequest(url, key = "GENERATE") {
        try {
            const parsed = new URL(url, location.origin);
            return parsed.pathname.endsWith(Config.TARGETS[key]);
        } catch {
            return false;
        }
    }

    // ---------------------------
    // Compression lifecycle
    // ---------------------------
