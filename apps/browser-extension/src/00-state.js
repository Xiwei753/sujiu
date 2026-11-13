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
            PRIOR_V3_FULL: "请只根据本次请求中实际提供的历史记录，制作供后续剧情使用的准确记忆摘要。按照事件发生顺序记录已完成的行动及后果、持续有效的人物关系和世界规则，以及记录末尾仍未解决的矛盾和伏笔。明确区分“已经结束的事件”和“在这些记录的末尾仍持续的状态”。所有状态都只截至本次提供的最后一条历史记录，不要推测未提供的后续剧情，也不要把过去情节描述成此刻正在发生。保留时间、地点、因果和重要关系的变化，忠实于原文，使用原对话语言，尽可能精炼。",
            PRIOR_V35_FULL: "你是负责长期会话连续性的记忆整理员。当前输入包含一段即将被归档的历史对话或故事记录。请把它整理成一份可供后续模型准确接续的长期记忆。以输入中真正提供的内容为全部证据，优先确保事实、时间先后、人物关系和状态变更准确。后续模型将另行取得更近的完整消息，因此这份记忆的时间终点仅是本次输入历史的最后一条记录。\n\n工作原则：\n- 区分「稳定不变的设定」「已经发生且结束的事实」「截至本段结尾仍持续的状态」「角色的打算或猜测」。角色说过的话、想象、谎言、误会、传闻与客观发生的事件分别归档。\n- 按发生顺序梳理事件，保持原因、经过、结果和后果。重点保留推动关系、目标、能力、资源、地点、时间、知识与权力状态改变的事实。历史上某个阶段正在进行的事情，应记录该阶段及其随后发生的结果；整段历史最后的状态只标注为「本段历史截点」，不代表整个会话此刻。\n- 遇到多个版本或互相冲突的叙述，列明出现过的版本和最后被明确确认或修正的版本，说明谁知道真相。无法确定的细节标记「未确定」，保留原文的相对时间顺序。\n- 既有压缩记忆如果已包含在输入中，把它当作更早时间段的历史索引；结合后续实际事件更新已变化的事实，避免将旧摘要的「当时」误读为现在，也避免把未经后续证实的推断升级为事实。\n- 按重要性分配篇幅：主要剧情、关键决策、角色心理与关系变化、长期约定、尚未兑现的伏笔可以写得细；重复闲聊和对后续没有影响的细节高度合并。可以使用较长摘要，完整性优先于固定字数。\n- 这是一项记录整理任务。输出内容应该是可检索的事实与状态，使用原会话的语言及惯用专名，不续写剧情、不代替人物作决定，也不推测尚未提供的后续情节。\n\n请以以下结构输出。没有依据的栏目可写「未提及」，无需填充猜想：\n\n【历史范围与时间锚点】\n明确本次输入所覆盖的时间段、起点和终点（仅在有依据时填写具体时间）。指出这份记录截至哪个场景或事件；此后发生的事情不在本摘要覆盖范围内。\n\n【稳定设定与约束】\n人物身份、称谓、外貌或能力中持续有效的关键特征；世界规则、地理、组织、限制、技术或魔法机制；用户明确约定的互动方式、目标、禁忌或长期偏好。区别固定设定和后来改动的设定。\n\n【人物与关系】\n逐个记录重要人物的身份、立场、已知信息、目标、心理倾向、相互关系；重点写清关系如何演变、谁向谁透露了什么、谁误解了什么、重要信任与矛盾的根源。区分人物私下想法与公开行动。\n\n【关键事件时间线】\n按先后顺序用具体事件作为条目。每项尽量包含「时间/地点（如明确）—相关人物—起因—行动/决定—结果—对后续的影响」。明确已结束事件的结局、尚在推进的过程及各个转折点；保留能解释后来行为的必要细节。\n\n【重要状态变化】\n概括已经发生的身份、关系、伤病与体能、所在地、装备与资源、持有物、权限、立场、任务进度等变化；写明变化前后，避免使用脱离时间点的“现在”“目前”指代过去。\n\n【约定、目标与待办】\n列出仍然有效的承诺、计划、交易、期限、责任归属和条件；已完成或已作废者注明处理结果。区分角色希望发生的事情与已经完成的事情。\n\n【未解决线索与悬而未决的信息】\n保留未解的疑问、秘密、伏笔、冲突、危险、尚未兑现的安排及各方知情范围。对于猜测或缺乏证据之处明确标记其不确定性。\n\n【本段历史截点】\n严格以本次输入最后已经确认的消息为边界，记录那一刻的人物位置、正在执行的动作、即时情绪或对话、仍在进行的事件及下一步可能需要衔接的事项。这里仅描述本段历史结束时的情形；整个长会话的实际最新状态应由另行保留的后续消息确定。\n\n【连续性校验】\n列出最容易被误记、混淆时间顺序或推翻设定的关键事实；尤其标注看似矛盾但有合理先后关系的情节，以及需保持原文一致的人名、日期、物品和规则。\n\n直接输出结构化记忆正文。事实准确、因果完整和时间线清晰优先。",
            FULL: "你是负责长期会话连续性的记忆整理员。当前输入包含截至这次摘要生成前的全部历史对话或故事记录。请把它整理成一份可供后续模型准确接续的长期记忆。以输入中真正提供的内容为全部证据，优先确保事实、时间先后、人物关系和状态变更准确。本次摘要应涵盖整段历史，包括最后的近期对话；它们的原文也会在后续清理时保留，以确保剧情衔接。\n\n工作原则：\n- 区分「稳定不变的设定」「已经发生且结束的事实」「截至本段结尾仍持续的状态」「角色的打算或猜测」。角色说过的话、想象、谎言、误会、传闻与客观发生的事件分别归档。\n- 按发生顺序梳理事件，保持原因、经过、结果和后果。重点保留推动关系、目标、能力、资源、地点、时间、知识与权力状态改变的事实。历史上某个阶段正在进行的事情，应记录该阶段及其随后发生的结果；整段历史最后的状态只标注为「本段历史截点」，不代表整个会话此刻。\n- 遇到多个版本或互相冲突的叙述，列明出现过的版本和最后被明确确认或修正的版本，说明谁知道真相。无法确定的细节标记「未确定」，保留原文的相对时间顺序。\n- 既有压缩记忆如果已包含在输入中，把它当作更早时间段的历史索引；结合后续实际事件更新已变化的事实，避免将旧摘要的「当时」误读为现在，也避免把未经后续证实的推断升级为事实。\n- 按重要性分配篇幅：主要剧情、关键决策、角色心理与关系变化、长期约定、尚未兑现的伏笔可以写得细；重复闲聊和对后续没有影响的细节高度合并。可以使用较长摘要，完整性优先于固定字数。\n- 这是一项记录整理任务。输出内容应该是可检索的事实与状态，使用原会话的语言及惯用专名，不续写剧情、不代替人物作决定，也不推测尚未提供的后续情节。\n\n请以以下结构输出。没有依据的栏目可写「未提及」，无需填充猜想：\n\n【历史范围与时间锚点】\n明确本次输入所覆盖的时间段、起点和终点（仅在有依据时填写具体时间）。指出这份记录截至哪个场景或事件；确认输入中最后的剧情发展与结局，避免误将早期事件判为最新事件。\n\n【稳定设定与约束】\n人物身份、称谓、外貌或能力中持续有效的关键特征；世界规则、地理、组织、限制、技术或魔法机制；用户明确约定的互动方式、目标、禁忌或长期偏好。区别固定设定和后来改动的设定。\n\n【人物与关系】\n逐个记录重要人物的身份、立场、已知信息、目标、心理倾向、相互关系；重点写清关系如何演变、谁向谁透露了什么、谁误解了什么、重要信任与矛盾的根源。区分人物私下想法与公开行动。\n\n【关键事件时间线】\n按先后顺序用具体事件作为条目。每项尽量包含「时间/地点（如明确）—相关人物—起因—行动/决定—结果—对后续的影响」。明确已结束事件的结局、尚在推进的过程及各个转折点；保留能解释后来行为的必要细节。\n\n【重要状态变化】\n概括已经发生的身份、关系、伤病与体能、所在地、装备与资源、持有物、权限、立场、任务进度等变化；写明变化前后，避免使用脱离时间点的“现在”“目前”指代过去。\n\n【约定、目标与待办】\n列出仍然有效的承诺、计划、交易、期限、责任归属和条件；已完成或已作废者注明处理结果。区分角色希望发生的事情与已经完成的事情。\n\n【未解决线索与悬而未决的信息】\n保留未解的疑问、秘密、伏笔、冲突、危险、尚未兑现的安排及各方知情范围。对于猜测或缺乏证据之处明确标记其不确定性。\n\n【本段历史截点】\n严格以本次输入最后已经确认的消息为边界，记录那一刻的人物位置、正在执行的动作、即时情绪或对话、仍在进行的事件及下一步可能需要衔接的事项。这里应准确描述截至本次摘要请求前的最新状态；后续保留的近期原文仍然是细节和衔接的优先依据，不要把更早阶段的状态冒充最后阶段。\n\n【连续性校验】\n列出最容易被误记、混淆时间顺序或推翻设定的关键事实；尤其标注看似矛盾但有合理先后关系的情节，以及需保持原文一致的人名、日期、物品和规则。\n\n直接输出结构化记忆正文。事实准确、因果完整和时间线清晰优先。",
            SNIPPET: "你是负责长期会话连续性的记忆整理员"
        }
    };

    const env = {
        isMac: /Mac|iPhone|iPad/.test(navigator.platform),
        isAppContext: location.pathname.startsWith("/app/"),
        topWindow: unsafeWindow?.top || window
    };
    const SCRIPT_VERSION = "0.4.14";

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
        keepRounds: 8
    };
    let userSettings = loadUserSettings();
    refreshActivePromptFromSettings();
