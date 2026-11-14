/** View-only renderer. All model-controlled text is HTML-escaped. */
export const DEFAULT_AVATARS = Object.freeze({
    '御坂美琴': '⚡', '白井黑子': '🎀', '食蜂操祈': '⭐',
    '初春饰利': '🌸', '佐天泪子': '🍀', '帆风润子': '🌙',
    '一方通行': '◈', '麦野沉利': '✦', '旁白': '📖',
    '老师': '📚', '班主任': '📚',
});

export function escapeHtml(value) {
    return String(value ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;')
        .replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

function richText(value) {
    const text = String(value ?? '').slice(0, 30_000);
    return text.split(/\n\s*\n/).map(p => `<p>${escapeHtml(p).replace(/\n/g, '<br>')}</p>`).join('');
}

function flatten(value, depth = 0) {
    if (value == null) return '';
    if (depth > 3) return '…';
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') return String(value);
    if (Array.isArray(value)) return value.map(v => flatten(v, depth + 1)).filter(Boolean).join('\n');
    if (typeof value === 'object') return Object.entries(value).slice(0, 60).map(([key, v]) => `${key}：${flatten(v, depth + 1)}`).join('\n');
    return '';
}

function avatarFor(name, custom) {
    const entry = custom && typeof custom === 'object' ? custom[name] : undefined;
    if (typeof entry === 'string' && entry.length < 10) return entry;
    return DEFAULT_AVATARS[name] ?? (Array.from(name)[0] || '·');
}

function colorFor(name) {
    let hash = 0;
    for (const c of name) hash = (hash * 31 + c.codePointAt(0)) >>> 0;
    return hash % 7;
}

function renderSegment(segment, avatars, continued = false) {
    const name = String(segment.speaker || '旁白');
    const kind = String(segment.kind || 'narration');
    const text = String(segment.text ?? '');
    if (!text.trim()) return '';
    if (kind === 'narration' || name === '旁白') {
        return `<section class="sujiu-narration" aria-label="场景旁白">${richText(text)}</section>`;
    }
    const nameSafe = escapeHtml(name);
    return `<section class="sujiu-speaker${continued ? ' sujiu-speaker-continued' : ''}" data-sujiu-kind="${escapeHtml(kind)}">
       <div class="sujiu-avatar sujiu-hue-${colorFor(name)}" aria-hidden="true">${continued ? '' : escapeHtml(avatarFor(name, avatars))}</div>
       <div class="sujiu-speaker-body">${continued ? '' : '<div class="sujiu-speaker-name">' + nameSafe + '</div>'}
       <div class="sujiu-speech">${richText(text)}</div></div></section>`;
}

function stateValue(v) { return flatten(v).replace(/\s+/g, ' ').trim(); }
function renderState(state) {
    if (!state) return '';
    const date = stateValue(state.time || state.date || '');
    const location = stateValue(state.location || '');
    const physical = stateValue(state.physical || state.body || '');
    const mental = stateValue(state.mental || state.ability || '');
    const inventory = stateValue(state.inventory || '');
    return `<section class="sujiu-status" aria-label="玩家状态">
      <div class="sujiu-status-title">状态</div><div>${[date, location, physical, mental].filter(Boolean).map(escapeHtml).join(' · ') || '待更新'}</div>
      <div class="sujiu-inventory"><strong>持有物品</strong> ${escapeHtml(inventory || '待确认')}</div></section>`;
}

function renderContinuity(record) {
    if (!record) return '';
    const raw = flatten(record).slice(0, 50_000);
    return `<details class="sujiu-continuity"><summary>sujiu · 剧情连续性记录</summary><div class="sujiu-continuity-text">${escapeHtml(raw)}</div></details>`;
}

export function renderSujiu(data, avatars = {}) {
    const parts = data.segments ?? [];
    const segments = parts.map((v, i) => renderSegment(v, avatars,
        i > 0 && v.speaker === parts[i - 1].speaker && v.kind === parts[i - 1].kind &&
        v.kind !== 'narration')).join('');
    const pending = data.complete ? '' : `<div class="sujiu-pending">${segments ? '生成中…' : '正在生成场景…'}</div>`;
    return `<div class="sujiu-scene" data-sujiu-version="4">${segments}${pending}${renderState(data.state)}${renderContinuity(data.continuity)}</div>`;
}
