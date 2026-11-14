import { parseSujiu } from './parser.mjs';
import { renderSujiu } from './renderer.mjs';

const MODULE = 'sujiu_structured_chat';
const DEFAULTS = { enabled: true, avatars: {} };
const context = () => globalThis.SillyTavern?.getContext?.();
let settings;
let hookReady = false;
let started = false;
let observer = null;
let processingFallback = false;
let queued = new Set();
let frame = 0;
let pendingSource = null;

function getSettings() {
    const ctx = context();
    if (!ctx?.extensionSettings) return settings || { ...DEFAULTS };
    const stored = ctx.extensionSettings[MODULE] ??= { ...DEFAULTS };
    for (const [key, value] of Object.entries(DEFAULTS)) if (!(key in stored)) stored[key] = value;
    settings = stored;
    return settings;
}

function isTarget() {
    const ctx = context();
    const current = ctx?.characters?.[ctx.characterId];
    return Boolean(getSettings().enabled && current?.data?.extensions?.sujiu?.structured_chat);
}

function canRender(meta) {
    return isTarget() && !meta?.isSystem && !meta?.isUser && !meta?.isReasoning;
}

function installFormattingHooks() {
    const formatter = context()?.messageFormatter;
    if (!formatter?.addHook || !formatter?.stage?.AFTER_MARKDOWN) return false;
    // The callbacks execute synchronously in one messageFormatting invocation.
    formatter.addHook((mes, meta) => {
        pendingSource = canRender(meta) ? { id: meta.messageId, raw: mes } : null;
        return mes;
    }, { stage: formatter.stage.AFTER_REGEX, order: formatter.order.LATEST });
    formatter.addHook((html, meta) => {
        if (!pendingSource || pendingSource.id !== meta.messageId || !canRender(meta)) return html;
        const raw = pendingSource.raw;
        pendingSource = null;
        const parsed = parseSujiu(raw);
        return parsed ? renderSujiu(parsed, getSettings().avatars) : html;
    }, { stage: formatter.stage.AFTER_MARKDOWN, order: formatter.order.LATEST });
    console.info('[sujiu-structured-chat] Native formatter mode');
    return true;
}

function renderFallbackMessage(messageElement) {
    if (!isTarget() || !messageElement?.isConnected) return;
    const textElement = messageElement.querySelector('.mes_text');
    if (!textElement) return;
    const id = Number(messageElement.getAttribute('mesid'));
    const chatItem = Number.isInteger(id) && id >= 0 ? context()?.chat?.[id] : null;
    if (chatItem?.is_user || chatItem?.is_system || messageElement.classList.contains('is_user')) return;
    if (textElement.closest('.mes_editing') || messageElement.querySelector('.edit_textarea, textarea.mes_edit')) return;
    // For completed messages use stored raw text, never DOM-derived rendered markup.
    // During streaming (which is not yet in chat), use visible text as a best-effort prefix.
    const domRaw = textElement.textContent || '';
    const raw = chatItem?.mes || domRaw;
    const current = textElement.querySelector('.sujiu-scene');
    if (current && textElement.dataset.sujiuSource === raw) return;
    // Do not re-parse our own rendered text when the source message is not available.
    if (current && !chatItem) return;
    const parsed = parseSujiu(raw);
    if (!parsed) return;
    processingFallback = true;
    try {
        textElement.innerHTML = renderSujiu(parsed, getSettings().avatars);
        textElement.dataset.sujiuSource = raw;
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
        if (processingFallback || !isTarget()) return;
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
    const panel = document.querySelector('#extensions_settings2');
    if (!panel || document.getElementById('sujiu-structured-settings')) return;
    const shell = document.createElement('section');
    shell.id = 'sujiu-structured-settings';
    shell.className = 'sujiu-settings';
    shell.innerHTML = `<details><summary>sujiu · 多角色气泡</summary>
      <label><input type="checkbox" id="sujiu-structured-toggle"> 启用结构化渲染</label>
      <label for="sujiu-avatar-map">头像符号映射（JSON，可选）</label>
      <textarea id="sujiu-avatar-map" rows="5" spellcheck="false" placeholder='{"御坂美琴":"⚡","白井黑子":"🎀"}'></textarea>
      <button id="sujiu-avatar-save" type="button">保存</button>
      <small>空白时使用内置符号，新增人物自动生成文字头像。仅影响显示，不修改消息。</small>
      </details>`;
    panel.append(shell);
    const enable = shell.querySelector('#sujiu-structured-toggle');
    const textarea = shell.querySelector('#sujiu-avatar-map');
    enable.checked = Boolean(getSettings().enabled);
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
    if (!hookReady) startFallback();
    mountSettings();
    const ctx = context();
    const listen = (type, fn) => { if (type && ctx?.eventSource?.on) ctx.eventSource.on(type, fn); };
    listen(ctx?.event_types?.CHAT_CHANGED, () => { if (!hookReady) startFallback(); });
    listen(ctx?.event_types?.APP_READY, mountSettings);
    listen(ctx?.event_types?.CHARACTER_MESSAGE_RENDERED, () => { if (!hookReady) document.querySelectorAll('#chat .mes').forEach(enqueueFallback); });
    console.info('[sujiu-structured-chat] loaded', hookReady ? 'formatter' : 'fallback');
}

// Stable API for current ST releases; also works on builds with APP_INITIALIZED firing late.
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', initialize, { once: true });
else initialize();
