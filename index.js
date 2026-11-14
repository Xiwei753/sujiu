import { parseSujiu, parseRoleplayText } from './parser.mjs';
import { renderSujiu } from './renderer.mjs';

const MODULE = 'sujiu_structured_chat';
const DEFAULTS = { enabled: true, plainTextFallback: true, avatars: {} };
const context = () => globalThis.SillyTavern?.getContext?.();
let settings;
let hookReady = false;
let started = false;
let observer = null;
let processingFallback = false;
let queued = new Set();
let frame = 0;
let pendingSources = new Map();
let lastResult = '尚未解析消息';

function getSettings() {
    const ctx = context();
    if (!ctx?.extensionSettings) return settings || { ...DEFAULTS };
    const stored = ctx.extensionSettings[MODULE] ??= { ...DEFAULTS };
    for (const [key, value] of Object.entries(DEFAULTS)) if (!(key in stored)) stored[key] = value;
    settings = stored;
    return settings;
}

function hasCardMarker() {
    const ctx = context();
    const current = ctx?.characters?.[ctx.characterId];
    return Boolean(current?.data?.extensions?.sujiu?.structured_chat ||
        current?.extensions?.sujiu?.structured_chat);
}
function isEnabled() { return Boolean(getSettings().enabled); }
function parseMessage(raw) {
    const json = parseSujiu(raw);
    if (json) return json;
    return getSettings().plainTextFallback ? parseRoleplayText(raw) : null;
}
function report(parsed, raw, id) {
    const format = parsed ? (parsed.format === 'plain' ? '多角色文本' : '结构化 JSON') : '原文（没有可识别说话人）';
    lastResult = format + (parsed ? ' · ' + parsed.segments.length + ' 段' : '') +
        (id === undefined || id === null ? '' : ' · #' + String(id));
    refreshDiagnostics();
}
function refreshDiagnostics() {
    const el = document.querySelector('#sujiu-structured-status');
    if (!el) return;
    el.textContent = '渲染模式：' + (hookReady ? '消息格式化 + DOM 兼容' : 'DOM 兼容') +
        '；角色卡标记：' + (hasCardMarker() ? '有' : '无（自动识别仍可用）') +
        '；最近结果：' + lastResult;
}
function canRender(meta) {
    return isEnabled() && !meta?.isSystem && !meta?.isUser && !meta?.isReasoning;
}

function installFormattingHooks() {
    const formatter = context()?.messageFormatter;
    if (!formatter?.addHook || !formatter?.stage?.AFTER_MARKDOWN || !formatter?.stage?.AFTER_REGEX) return false;
    const order = formatter.order?.LATEST;
    formatter.addHook((mes, meta) => {
        if (canRender(meta)) pendingSources.set(meta?.messageId, mes);
        return mes;
    }, { stage: formatter.stage.AFTER_REGEX, order });
    formatter.addHook((html, meta) => {
        const key = meta?.messageId;
        if (!pendingSources.has(key)) return html;
        const raw = pendingSources.get(key);
        pendingSources.delete(key);
        if (!canRender(meta)) return html;
        const parsed = parseMessage(raw);
        report(parsed, raw, key);
        return parsed ? renderSujiu(parsed, getSettings().avatars) : html;
    }, { stage: formatter.stage.AFTER_MARKDOWN, order });
    console.info('[sujiu-structured-chat] Native formatter mode');
    return true;
}

function renderFallbackMessage(messageElement) {
    if (!isEnabled() || !messageElement?.isConnected) return;
    const textElement = messageElement.querySelector('.mes_text');
    if (!textElement) return;
    const idAttr = messageElement.getAttribute('mesid');
    const id = idAttr === null ? -1 : Number(idAttr);
    const chatItem = Number.isInteger(id) && id >= 0 ? context()?.chat?.[id] : null;
    // Never overwrite a partial stream: ST is still writing into its own DOM.
    if (!chatItem || typeof chatItem.mes !== 'string' || !chatItem.mes.trim()) return;
    if (chatItem.is_user || chatItem.is_system || messageElement.classList.contains('is_user')) return;
    if (textElement.closest('.mes_editing') || messageElement.querySelector('.edit_textarea, textarea.mes_edit')) return;
    const raw = chatItem.mes;
    const current = textElement.querySelector('.sujiu-scene');
    // A native MessageFormatter-rendered scene has no fallback source stamp.
    if (current && !textElement.dataset.sujiuSource) return;
    if (current && textElement.dataset.sujiuSource === raw) return;
    const parsed = parseMessage(raw);
    if (!parsed) {
        if (textElement.dataset.sujiuSource) {
            // A user edit or changed setting may invalidate the fallback parse.
            // Restore the stored source safely rather than leaving stale bubbles.
            processingFallback = true;
            try {
                textElement.textContent = raw;
                textElement.style.whiteSpace = 'pre-wrap';
                delete textElement.dataset.sujiuSource;
            } finally { processingFallback = false; }
        }
        report(null, raw, id);
        return;
    }
    processingFallback = true;
    try {
        textElement.innerHTML = renderSujiu(parsed, getSettings().avatars);
        textElement.dataset.sujiuSource = raw;
        report(parsed, raw, id);
    } finally { processingFallback = false; }
}

function drainFallbackQueue() {
    frame = 0;
    const batch = [...queued];
    queued.clear();
    for (const el of batch) renderFallbackMessage(el);
}
function enqueueFallback(el) {
    if (!el) return;
    queued.add(el);
    if (!frame) frame = requestAnimationFrame(drainFallbackQueue);
}
function startFallback() {
    const root = document.querySelector('#chat');
    if (!root) return;
    observer?.disconnect();
    observer = new MutationObserver(records => {
        if (processingFallback || !isEnabled()) return;
        for (const record of records) {
            const el = record.target.nodeType === 1 ? record.target : record.target.parentElement;
            enqueueFallback(el?.closest?.('.mes'));
            for (const node of record.addedNodes) {
                if (node.nodeType !== 1) continue;
                if (node.matches?.('.mes')) enqueueFallback(node);
                node.querySelectorAll?.('.mes').forEach(enqueueFallback);
            }
        }
    });
    observer.observe(root, { subtree: true, characterData: true, childList: true });
    root.querySelectorAll('.mes').forEach(enqueueFallback);
    console.info('[sujiu-structured-chat] Legacy DOM fallback mode');
}

function mountSettings() {
    const panel = document.querySelector('#extensions_settings2') || document.querySelector('#extensions_settings');
    if (!panel || document.getElementById('sujiu-structured-settings')) return;
    const shell = document.createElement('section');
    shell.id = 'sujiu-structured-settings';
    shell.className = 'sujiu-settings';
    shell.innerHTML = `<details><summary>sujiu · 多角色气泡</summary>
      <label><input type="checkbox" id="sujiu-structured-toggle"> 启用多角色气泡</label>
      <label><input type="checkbox" id="sujiu-plain-toggle"> 兼容识别普通文本中的角色台词</label>
      <div class="sujiu-diagnostics" id="sujiu-structured-status" role="status"></div>
      <label for="sujiu-avatar-map">头像符号映射（JSON，可选）</label>
      <textarea id="sujiu-avatar-map" rows="5" spellcheck="false" placeholder='{"御坂美琴":"⚡","白井黑子":"🎀"}'></textarea>
      <button id="sujiu-avatar-save" type="button">保存</button>
      <small>空白时使用内置符号，新增人物自动生成文字头像。仅影响显示，不修改消息。</small>
      </details>`;
    panel.append(shell);
    const enable = shell.querySelector('#sujiu-structured-toggle');
    const textarea = shell.querySelector('#sujiu-avatar-map');
    enable.checked = Boolean(getSettings().enabled);
    const plain = shell.querySelector('#sujiu-plain-toggle');
    plain.checked = Boolean(getSettings().plainTextFallback);
    refreshDiagnostics();
    plain.addEventListener('change', () => {
        getSettings().plainTextFallback = plain.checked;
        context()?.saveSettingsDebounced?.();
        context()?.reloadCurrentChat?.();
    });
    textarea.value = Object.keys(getSettings().avatars).length ? JSON.stringify(getSettings().avatars, null, 2) : '';
    enable.addEventListener('change', () => {
        getSettings().enabled = enable.checked;
        context()?.saveSettingsDebounced?.();
        context()?.reloadCurrentChat?.();
    });
    shell.querySelector('#sujiu-avatar-save').addEventListener('click', () => {
        try {
            const map = textarea.value.trim() ? JSON.parse(textarea.value) : {};
            if (!map || Array.isArray(map) || typeof map !== 'object') throw new Error('需要 JSON 对象');
            const values = Object.entries(map).slice(0, 200).filter(([key, value]) => key.length < 80 && typeof value === 'string' && value.length < 10);
            getSettings().avatars = Object.fromEntries(values);
            context()?.saveSettingsDebounced?.();
            context()?.reloadCurrentChat?.();
        } catch (e) { alert('头像映射格式有误：' + e.message); }
    });
}

function initialize() {
    if (started) return;
    started = true;
    getSettings();
    hookReady = installFormattingHooks();
    startFallback();
    mountSettings();
    const ctx = context();
    const listen = (type, fn) => { if (type && ctx?.eventSource?.on) ctx.eventSource.on(type, fn); };
    const events = ctx?.eventTypes ?? ctx?.event_types;
    listen(events?.CHAT_CHANGED, () => { pendingSources.clear(); lastResult = '尚未解析消息'; startFallback(); refreshDiagnostics(); });
    listen(events?.APP_READY, mountSettings);
    listen(events?.CHARACTER_MESSAGE_RENDERED, () => { document.querySelectorAll('#chat .mes').forEach(enqueueFallback); });
    listen(events?.MESSAGE_RECEIVED, () => { document.querySelectorAll('#chat .mes').forEach(enqueueFallback); });
    console.info('[sujiu-structured-chat] loaded', hookReady ? 'formatter' : 'fallback');
}

// Stable API for current ST releases; also works on builds with APP_INITIALIZED firing late.
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', initialize, { once: true });
else initialize();
