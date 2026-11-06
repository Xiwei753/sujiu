// Adapted from lxchx/aistudio-compressor v0.3.2 (MIT); see THIRD_PARTY_NOTICES.md.
    const Config = {
        TAGS: {
            UI: "[Compressor UI]",
            NET: "[Compressor Monitor]"
        },
        TARGETS: {
            GENERATE: "/$rpc/google.internal.alkali.applications.makersuite.v1.MakerSuiteService/GenerateContent",
            RESOLVE: "/$rpc/google.internal.alkali.applications.makersuite.v1.MakerSuiteService/ResolveDriveResource",
            UPDATE: "/$rpc/google.internal.alkali.applications.makersuite.v1.MakerSuiteService/UpdatePrompt"
        },
        NETWORK: {
            RPC_ORIGIN: "https://alkalimakersuite-pa.clients6.google.com"
        },
        EVENTS: {
            REQUEST: "aistudio-compressor:generatecontent-request",
            RESPONSE: "aistudio-compressor:generatecontent-response"
        },
        UI: {
            BUTTON_ID: "compressor-button-hybrid",
            TOOLBAR_SELECTORS: [
                "ms-toolbar .toolbar-right",
                "ms-playground-toolbar .toolbar-right",
                "ms-playground-toolbar .toolbar-container .toolbar-right",
                ".toolbar-right"
            ],
            TARGET_PATH: "/prompts/",
            INPUT_SELECTORS: [
                "ms-autosize-textarea textarea",
                'textarea[aria-label="Start typing a prompt"]',
                "textarea"
            ],
            SEND_BUTTON_SELECTORS: [
                "ms-run-button button",
                "ms-run-button button.run-button",
                'button[aria-label="Run"]',
                'button[aria-label^="Run"]',
                'button[description*="Send prompt"]',
                'button[aria-label*="keyboard_return"]',
                'button[type="submit"]'
            ]
        },
        PROMPTS: {
            LEGACY_FULL: "Summarize the earlier role-play conversation as a compact, accurate memory snapshot.\nOnly summarize the history BEFORE the most recent completed exchanges identified in the final instruction. Preserve established characters, relationships, identity, world rules, important events, state changes, promises, possessions, unresolved conflicts, and the current situation before those exchanges. Avoid adding fictional details or overriding established facts.\nReturn only:\n<state_snapshot>\n[Concise, structured summary in the language of the conversation]\n</state_snapshot>",
            PREVIOUS_FULL: "请为这段长期对话制作按剧情时间线整理的精炼记忆，用于之后继续对话。仅总结后附说明指定的较早轮次。\n\n按以下顺序组织：\n【稳定设定】人物身份、关系、世界规则、持续有效的约定。\n【已发生的关键事件】按先后顺序记录重要行动、变化和结果；已结束的事件使用完成时态，标明结局和后续影响。\n【截至压缩截点的状态】只描述待删除历史结束时已经确立的状态、尚未解决的矛盾和后续伏笔，明确这是“压缩截点”而非整段对话的现在。\n【时间线衔接】说明最近保留轮次将接在什么事件之后。真正正在发生的剧情、地点和时间，应由保留的最近轮次决定。\n\n优先保留后续会影响人物决策和剧情因果的细节；输出应清晰、紧凑、忠于原文。仅返回上述结构化记忆，使用原对话语言。",
            FULL: "请只根据本次请求中实际提供的历史记录，制作供后续剧情使用的准确记忆摘要。按照事件发生顺序记录已完成的行动及后果、持续有效的人物关系和世界规则，以及记录末尾仍未解决的矛盾和伏笔。明确区分“已经结束的事件”和“在这些记录的末尾仍持续的状态”。所有状态都只截至本次提供的最后一条历史记录，不要推测未提供的后续剧情，也不要把过去情节描述成此刻正在发生。保留时间、地点、因果和重要关系的变化，忠实于原文，使用原对话语言，尽可能精炼。",
            SNIPPET: "请为这段长期对话制作按剧情时间线整理的精炼记忆"
        }
    };

    const env = {
        isMac: /Mac|iPhone|iPad/.test(navigator.platform),
        isAppContext: location.pathname.startsWith("/app/"),
        topWindow: unsafeWindow?.top || window
    };
    const SCRIPT_VERSION = "0.3.2";

    const state = {
        compressionInProgress: false,
        compressionRequestPending: false,
        compressionResponsePending: false,
        lastPromptHistory: null,
        lastPromptHistoryUpdatedAt: 0,
        lastResolvedPromptThread: null,
        lastResolvedPromptRoot: null,
        lastResolvedPromptId: null,
        lastResolvedPromptUpdatedAt: 0,
        googleApiKey: "",
        googleAuthUser: "0",
        googleApiKeyUpdatedAt: 0,
        historyCapturePending: false,
        branchInProgress: false,
        activeCompressionPrompt: "",
        activeCompressionMarker: "",
        activeCompressionKeepRounds: 0,
        scopedCompression: null,
        activeCompressionSnippet: Config.PROMPTS.SNIPPET
    };

    const log = {
        ui: (...args) => console.log(Config.TAGS.UI, ...args),
        net: (...args) => console.log(Config.TAGS.NET, ...args)
    };

    const STORAGE_KEY = "__aistudio_compressor_settings_v1";
    const DEFAULT_SETTINGS = {
        compressPrompt: Config.PROMPTS.FULL,
        snapshotRegex: "",
        keepRounds: 8,
        autoInject: false
    };
    let userSettings = loadUserSettings();
    refreshActivePromptFromSettings();
