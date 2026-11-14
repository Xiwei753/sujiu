/** Pure parser for sujiu's single-message, multi-speaker output.
 * No network, DOM, or application state. Deliberately tolerant of a streaming JSON prefix.
 */
const MAX_RAW = 250_000;
const MAX_SEGMENTS = 120;
const MAX_SEGMENT_TEXT = 24_000;

function candidate(raw) {
    if (typeof raw !== 'string') return null;
    let value = raw.trimStart().replace(/^\uFEFF/, '');
    if (value.startsWith('```')) {
        value = value.replace(/^```(?:json|JSON)?\s*\n?/, '').replace(/\s*```\s*$/, '');
    }
    // Allow a very short accidental preface before an otherwise valid JSON object.
    if (!value.startsWith('{')) {
        const pos = value.indexOf('{');
        if (pos < 0 || pos > 120 || !/"(?:sujiu_schema|segments)"/.test(value.slice(pos, pos + 500))) return null;
        value = value.slice(pos);
    }
    return value.slice(0, MAX_RAW);
}

function readJSONStringFragment(value, start) {
    let raw = '';
    let escaped = false;
    for (let i = start; i < value.length; i++) {
        const char = value[i];
        if (char === '"' && !escaped) return { value: decodeJSONString(raw), finished: true };
        raw += char;
        if (char === '\\') escaped = !escaped;
        else escaped = false;
    }
    return { value: decodeJSONString(raw), finished: false };
}

function decodeJSONString(fragment) {
    try { return JSON.parse('"' + fragment + '"'); }
    catch {
        // Incomplete streaming escapes are common near token boundaries.
        return fragment.replace(/\\n/g, '\n').replace(/\\t/g, '\t')
            .replace(/\\r/g, '\r').replace(/\\"/g, '"').replace(/\\\\/g, '\\');
    }
}

function parsePartialSegment(fragment) {
    const matchField = (field) => {
        const match = new RegExp('"' + field + '"\\s*:\\s*"').exec(fragment);
        return match ? readJSONStringFragment(fragment, match.index + match[0].length).value : '';
    };
    const speaker = matchField('speaker');
    const text = matchField('text');
    if (!speaker && !text) return null;
    return { speaker: speaker || '旁白', kind: matchField('kind') || 'narration', text };
}

function safeSegment(segment) {
    if (!segment || typeof segment !== 'object') return null;
    const speaker = String(segment.speaker ?? '旁白').trim().slice(0, 80) || '旁白';
    const kind = String(segment.kind ?? 'dialogue').trim().slice(0, 24);
    const text = String(segment.text ?? '').slice(0, MAX_SEGMENT_TEXT);
    return { speaker, kind, text };
}

function normalizeComplete(value) {
    if (!value || typeof value !== 'object' || !Array.isArray(value.segments)) return null;
    if (value.sujiu_schema !== '1' && value.sujiu_schema !== 1 && value.sujiu_schema !== '1.0') return null;
    return {
        complete: true,
        segments: value.segments.slice(0, MAX_SEGMENTS).map(safeSegment).filter(Boolean),
        state: value.state && typeof value.state === 'object' ? value.state : null,
        continuity: value.continuity && typeof value.continuity === 'object' ? value.continuity : null,
    };
}

/** Scan balanced segment objects up to the currently streamed cursor. */
function partialSegments(value) {
    const m = /"segments"\s*:\s*\[/.exec(value);
    if (!m) return [];
    let i = m.index + m[0].length;
    let start = -1;
    let depth = 0;
    let insideString = false;
    let escape = false;
    const entries = [];
    for (; i < value.length && entries.length < MAX_SEGMENTS; i++) {
        const ch = value[i];
        if (insideString) {
            if (ch === '"' && !escape) insideString = false;
            if (ch === '\\') escape = !escape;
            else escape = false;
            continue;
        }
        if (ch === '"') { insideString = true; continue; }
        if (ch === '{') { if (depth === 0) start = i; depth++; continue; }
        if (ch === '}' && depth > 0) {
            depth--;
            if (depth === 0 && start >= 0) {
                const sub = value.slice(start, i + 1);
                try { entries.push(safeSegment(JSON.parse(sub))); }
                catch { const parsed = parsePartialSegment(sub); if (parsed) entries.push(parsed); }
                start = -1;
            }
        }
        if (ch === ']' && depth === 0) break;
    }
    if (depth > 0 && start >= 0 && entries.length < MAX_SEGMENTS) {
        const unfinished = parsePartialSegment(value.slice(start));
        if (unfinished) entries.push(unfinished);
    }
    return entries.filter(Boolean);
}

/**
 * @param {string} raw Raw model message (not ST-rendered HTML)
 * @returns {{complete:boolean,segments:Array,state:object|null,continuity:object|null}|null}
 */
export function parseSujiu(raw) {
    const value = candidate(raw);
    if (value === null) return null;
    try {
        const parsed = JSON.parse(value);
        // A complete unrelated JSON message should stay in SillyTavern's normal renderer.
        return normalizeComplete(parsed);
    } catch { /* Streaming JSON is expected to be incomplete. */ }
    // During the first few tokens the schema identifier might not be complete yet.
    // Once the output grows, require a recognizable sujiu field.
    if (!/^\s*\{/.test(value)) return null;
    if (value.length > 100 && !/"(?:sujiu_schema|segments)"/.test(value.slice(0, 1200))) return null;
    return { complete: false, segments: partialSegments(value), state: null, continuity: null };
}
