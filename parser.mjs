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


// Deliberately conservative fallback for multi-speaker, human-readable RP.
const ROLE_LABEL = /^(?:老师|班主任|教官|导师|同学[\p{L}\p{N}]{0,10}|学生[\p{L}\p{N}]{0,10}|学生会长|风纪委员|旁白)$/u;
const COMMON_NAMES = new Set(['御坂美琴','白井黑子','食蜂操祈','初春饰利','佐天泪子','帆风润子','一方通行','麦野沉利']);
const NON_NAMES = new Set(['时间','地点','状态','持有物品','场景','剧情','备注','今日','清晨','黄昏','翌日','教室','走廊','窗边','讲台','座位','下午','晚上','上午']);
const EMOJI_LINE = /^(?:[\u{1F000}-\u{1FAFF}\u2600-\u27BF]\uFE0F?\s*){1,4}$/u;
function labelInfo(line) {
    let name = line.trim(), marked = false;
    if ((name.startsWith('**') && name.endsWith('**')) || (name.startsWith('__') && name.endsWith('__'))) {
        name = name.slice(2, -2).trim(); marked = true;
    }
    if ((name.startsWith('【') && name.endsWith('】')) || (name.startsWith('[') && name.endsWith(']')) ||
        (name.startsWith('「') && name.endsWith('」'))) {
        name = name.slice(1, -1).trim(); marked = true;
    }
    return {name, marked};
}
function speakerName(line, next = '', avatar = false) {
    const {name, marked} = labelInfo(line);
    if (!name || name.length > 24 || NON_NAMES.has(name)) return null;
    if (ROLE_LABEL.test(name) || COMMON_NAMES.has(name)) return name;
    if (/^[\p{Script=Han}·]{2,8}$/u.test(name) &&
        (marked || avatar || /^[「『“"'‘]/u.test(next.trim()))) return name;
    if (/^[A-Za-z][A-Za-z0-9 _.·-]{1,19}$/u.test(name) &&
        (marked || avatar || /^[「『“"'‘]/u.test(next.trim()))) return name;
    return null;
}
function nextContent(lines, start) {
    for (let i=start; i<lines.length; i++) {
        const value=lines[i].trim();
        if (value && !EMOJI_LINE.test(value)) return value;
    }
    return '';
}
function likelyNarration(line) {
    return /^(?:窗边|旁边|一旁|门口|教室|讲台|走廊|过道|座位|这时|此时|随后|与此同时|你从|她|他|班主任|老师|御坂美琴|食蜂操祈)/u.test(line) &&
        /(?:轻轻|已经|抬|挑|点了|回头|望|看|朝|走|站|坐|露出|扫过|指向|记号|补充|沿着|微笑|皱眉|转身|低头|悄悄)/u.test(line);
}
/** Only parse clear speaker boundaries. Return null for ordinary prose. */
export function parseRoleplayText(raw) {
    if (typeof raw !== 'string' || !raw.trim() || raw.length > MAX_RAW) return null;
    const source=raw.trim();
    if (/^(?:\{|\[|[\x60]{3}\s*(?:json)?\s*[\r\n]*\{)/i.test(source)) return null;
    const lines=raw.replace(/\r\n?/g, '\n').split('\n');
    if (lines.length>1800) return null;
    const segments=[];
    let active=null, labels=0;
    const flush=()=>{
        if (!active) return;
        active.text=active.text.trim().slice(0,MAX_SEGMENT_TEXT);
        if (active.text) segments.push(active);
        active=null;
    };
    const append=(speaker,kind,value)=>{
        if (active && active.speaker===speaker && active.kind===kind) active.text+=(active.text?'\n':'')+value;
        else {flush(); active={speaker,kind,text:value};}
    };
    for(let i=0;i<lines.length && segments.length<MAX_SEGMENTS;i++){
        const line=lines[i].trim();
        if(!line){if(active?.text) active.text+='\n';continue;}
        const next=nextContent(lines,i+1);
        if(line==='同' && /^同学[\p{L}\p{N}]{1,10}$/u.test(next)) continue;
        if(EMOJI_LINE.test(line) && speakerName(next,nextContent(lines,i+2),true)) continue;
        const inline=/^(.{1,24}?)\s*[：:]\s*(.+)$/u.exec(line);
        if(inline){
            const speaker=speakerName(inline[1],inline[2],true);
            if(speaker){labels++;append(speaker,speaker==='旁白'?'narration':'dialogue',inline[2]);continue;}
        }
        const avatar=EMOJI_LINE.test(lines[i-1]?.trim()||'');
        const speaker=speakerName(line,next,avatar);
        if(speaker && next && !/^[：:。！？,.，；;]/u.test(next)){
            labels++;flush();active={speaker,kind:speaker==='旁白'?'narration':'dialogue',text:''};continue;
        }
        if(active?.kind==='dialogue' && /[。！？!?」』”]\s*$/u.test(active.text.trim()) && likelyNarration(line)) flush();
        append(active?.speaker||'旁白',active?.kind||'narration',line);
    }
    flush();
    if(!labels || !segments.some(s=>s.kind==='dialogue'))return null;
    return {complete:true,format:'plain',segments,state:null,continuity:null};
}
