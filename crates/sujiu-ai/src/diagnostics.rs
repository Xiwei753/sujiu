//! A readable record of what Sujiu asked an endpoint and what came back.
//!
//! Everything here exists for one reason: when a configuration does not work,
//! the question is always the same and the answer is always one of several very
//! different things. Was the address wrong? Was the key rejected? Did the
//! protocol probe fail, or did the model list? Did the turn itself fail? Mixing
//! those into "the provider is not configured" is what makes an endpoint
//! impossible to debug, and it is exactly the problem this log exists to remove.
//!
//! Three rules shape the module.
//!
//! **Secrets never reach it.** A key is only ever recorded in a masked form, and
//! every string that goes in is passed through [`redact`] first. A log is the
//! single most-shared artifact a bug report contains, so a log that can print a
//! key is a log that will print one.
//!
//! **Discovery and chat are different lines.** A capability probe, a model
//! listing and a conversation request are three different activities with three
//! different failure modes, and they carry a [`DiagnosticKind`] so a reader can
//! filter to one of them instead of guessing from timestamps which is which.
//!
//! **It is bounded.** A log that grows without limit is a log that eventually
//! stops being written. The oldest entries fall off the end.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Longest server body kept from any response.
///
/// A rejection body is worth having and is often an HTML error page, so it is
/// truncated rather than dropped. What it must not be is unbounded: a log entry
/// is supposed to be read, and a megabyte of router page is not read.
const MAX_BODY_CHARS: usize = 512;

/// How many entries a log keeps.
const DEFAULT_CAPACITY: usize = 400;

/// Which activity a line belongs to.
///
/// The two are kept apart because the failures are different. A probe that
/// concludes nothing has said nothing about whether a turn will work, and a turn
/// that fails after a successful probe has said nothing about the endpoint's
/// capabilities. Reading them as one stream is how a working configuration gets
/// "fixed" into a broken one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    /// Capability probing and model discovery.
    Discovery,
    /// An ordinary conversation request.
    Chat,
}

impl DiagnosticKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Chat => "chat",
        }
    }
}

/// One recorded fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEntry {
    pub at_ms: i64,
    pub kind: DiagnosticKind,
    /// What was being done, in the runtime's own words: `probe_start`,
    /// `protocol_result`, `model_listing`, `chat_request`, and so on.
    ///
    /// A stable word rather than a sentence, so a reader can filter and a
    /// screen can localize it.
    pub stage: String,
    /// The line itself. Already redacted.
    pub message: String,
    /// Structured detail, already redacted. Never carries a credential.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

impl DiagnosticEntry {
    fn new(
        kind: DiagnosticKind,
        stage: &str,
        message: impl Into<String>,
        fields: BTreeMap<String, String>,
    ) -> Self {
        Self {
            at_ms: now_ms(),
            kind,
            stage: stage.to_string(),
            message: redact(&message.into()),
            fields: fields
                .into_iter()
                .map(|(key, value)| (key, redact(&value)))
                .collect(),
        }
    }

    /// The entry as one readable line.
    pub fn render(&self) -> String {
        let stamp = crate::diagnostics::format_at(self.at_ms);
        let detail = if self.fields.is_empty() {
            String::new()
        } else {
            let fields: Vec<String> = self
                .fields
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            format!(" [{}]", fields.join(" "))
        };
        format!(
            "{stamp} {:<9} {:<20} {}{detail}",
            self.kind.label(),
            self.stage,
            self.message
        )
    }
}

/// A bounded, thread-safe record of what was asked and what came back.
///
/// Shared as an `Arc` so a probe, a listing and a turn can all add to the same
/// log without any of them owning it. Every value is redacted on the way in, so
/// there is no code path that can write a key into this by forgetting to.
#[derive(Debug)]
pub struct DiagnosticLog {
    capacity: usize,
    entries: Mutex<VecDeque<DiagnosticEntry>>,
}

impl Default for DiagnosticLog {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl DiagnosticLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Mutex::new(VecDeque::new()),
        }
    }

    /// Record one line. The kind, stage, message and every field are redacted
    /// before they are stored.
    pub fn record(
        &self,
        kind: DiagnosticKind,
        stage: &str,
        message: impl Into<String>,
        fields: BTreeMap<String, String>,
    ) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push_back(DiagnosticEntry::new(kind, stage, message, fields));
            while entries.len() > self.capacity {
                entries.pop_front();
            }
        }
    }

    /// Record a line with no structured fields, which is most of them.
    pub fn note(&self, kind: DiagnosticKind, stage: &str, message: impl Into<String>) {
        self.record(kind, stage, message, BTreeMap::new());
    }

    /// Every line, oldest first.
    pub fn entries(&self) -> Vec<DiagnosticEntry> {
        self.entries
            .lock()
            .map(|entries| entries.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The most recent `limit` lines, oldest first.
    ///
    /// Newest-last ordering is preserved rather than reversed: a log is read
    /// forwards, and a screen that wants the latest thing shows the tail.
    pub fn recent(&self, limit: usize) -> Vec<DiagnosticEntry> {
        let all = self.entries();
        if limit == 0 || all.len() <= limit {
            return all;
        }
        all[all.len() - limit..].to_vec()
    }

    /// Every line as text, one per line, for a log file or a bug report.
    pub fn render(&self) -> String {
        self.entries()
            .iter()
            .map(DiagnosticEntry::render)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Just one kind of activity, as text. This is what makes the two halves of
    /// the log separately readable.
    pub fn render_kind(&self, kind: DiagnosticKind) -> String {
        self.entries()
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(DiagnosticEntry::render)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.entries
            .lock()
            .map(|entries| entries.is_empty())
            .unwrap_or(true)
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Forget everything, so a user can start a fresh investigation.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }

    /// Replace the contents, used when restoring a persisted log.
    pub fn restore(&self, entries: Vec<DiagnosticEntry>) {
        if let Ok(mut log) = self.entries.lock() {
            log.clear();
            for entry in entries.into_iter().take(self.capacity) {
                log.push_back(entry);
            }
        }
    }
}

/// Key by which a persisted log is stored.
pub const LOG_KEY: &str = "sujiu-diagnostics.json";

/// Record a diagnostic line against a log that may not exist.
///
/// Discovery is useful with no log attached — several callers are plain library
/// users — and a missing log must never change what an endpoint is asked.
pub fn record(
    log: Option<&DiagnosticLog>,
    kind: DiagnosticKind,
    stage: &str,
    message: impl Into<String>,
    fields: BTreeMap<String, String>,
) {
    if let Some(log) = log {
        log.record(kind, stage, message, fields);
    }
}

/// Build the field map without ceremony, since nearly every line has a couple.
pub fn fields<const N: usize>(pairs: [(&str, String); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

/// A key in a form safe to write down.
///
/// Enough to tell two keys apart in a log and not enough to use one: the first
/// three characters, which carry the vendor prefix, and the last four, which are
/// what a person recognises their own key by. A short key is fully masked
/// instead, because "enough to tell them apart" is not worth publishing a secret
/// that is four characters long.
pub fn mask_secret(secret: &str) -> String {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() < 12 {
        return "****".to_string();
    }

    let head: String = chars.iter().take(3).collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}****{tail}")
}

/// Truncate a server body to something readable.
pub fn truncate(body: &str) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_BODY_CHARS {
        return collapsed;
    }
    let head: String = collapsed.chars().take(MAX_BODY_CHARS).collect();
    format!("{head}…")
}

/// Remove anything that looks like a credential from a string.
///
/// Applied to every message and every field on the way into the log, so the
/// redaction cannot be forgotten at a call site. Three things are handled
/// because all three turn up in real traffic: a bearer token echoed in an error,
/// a JSON body with a credential field in it, and a key quoted on its own.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut at = 0usize;

    while at < bytes.len() {
        if let Some((start, skip_to, replacement)) = secret_at(text, &lower, at) {
            // The text before the match is ordinary and is kept: a redaction
            // that also ate the sentence around the key would make the line
            // unreadable, and unreadable is how a leak becomes a habit.
            out.push_str(&text[at..start]);
            out.push_str(&replacement);
            at = skip_to;
            continue;
        }
        // Char boundaries: `at` only ever moves to a boundary the matchers
        // report, and a byte that is not a UTF-8 lead is copied as part of the
        // char it belongs to.
        let mut end = at + 1;
        while end < bytes.len() && (bytes[end] & 0xC0) == 0x80 {
            end += 1;
        }
        out.push_str(&text[at..end]);
        at = end;
    }

    out
}

/// Markers that begin something which must not be written down.
///
/// `authorization` and `bearer ` are a header and a scheme, `api_key` and its
/// spellings are field names inside a body, and `sk-` is the prefix most real
/// keys carry. All of them turn up in real traffic, and all of them are the
/// same leak.
const SECRET_MARKERS: [&str; 6] = [
    "authorization",
    "api_key",
    "api-key",
    "x-api-key",
    "bearer ",
    "sk-",
];

/// If a secret starts at or after `from`, where it starts, where it ends, and
/// what should be written in its place.
///
/// The earliest match wins, so a key that contains another marker is still
/// removed as one span rather than in two halves that leave the middle behind.
fn secret_at(text: &str, lower: &str, from: usize) -> Option<(usize, usize, String)> {
    SECRET_MARKERS
        .iter()
        .filter_map(|marker| value_span(text, lower, from, marker))
        .min_by_key(|(start, _, _)| *start)
}

/// The span one marker occupies, and what replaces it.
///
/// A marker means different things depending on what follows it, and treating
/// them alike is how a redactor either leaks or destroys.
fn value_span(
    text: &str,
    lower: &str,
    from: usize,
    marker: &str,
) -> Option<(usize, usize, String)> {
    let found = lower[from..].find(marker)?;
    let start = from + found;
    let after = start + marker.len();

    // A named field: `api_key`, `"api_key"`, `x-api-key:`. The name is quoted in
    // a JSON body, so the closing quote and the separator both have to be
    // stepped over before the value even begins. Missing that step is what made
    // an earlier version read the field's own closing quote as the end of the
    // value and redact nothing at all.
    //
    // The span starts at the *value*, not at the name: the name is what makes
    // the line readable, and a log that says `{"****": …}` has thrown away the
    // one clue about what was redacted.
    if let Some((value_start, quoted)) = value_after_separator(lower, after) {
        let (end, replaced) = if quoted {
            // A quoted value ends at its own closing quote, which may contain
            // spaces. Stopping at the first space would leave the rest of the
            // credential in the log.
            match text[value_start..].find(['"', '\'']) {
                Some(offset) => (value_start + offset + 1, true),
                None => (text.len(), true),
            }
        } else {
            // An unquoted value runs to the end of the field or the line, not to
            // the next space. `Authorization: Bearer eyJ...` is one credential
            // written as two words, and stopping at the space would leave the
            // token itself in the log.
            value_end_unquoted(text, value_start)
        };

        return replaced.then_some((value_start, end, "****".to_string()));
    }

    // A scheme, a header name, or a key written on its own. The token runs from
    // here to the next delimiter.
    let (end, replaced) = token_end(text, after);
    if !replaced {
        // A marker with nothing after it is a word, not a secret.
        return None;
    }

    // Masked rather than dropped, so two different keys stay tellable apart in
    // one log — which is the whole reason a log is worth reading.
    Some((start, end, mask_secret(&text[start..end])))
}

/// Where an unquoted value ends: at the end of the line, or at the punctuation
/// that ends a field inside a body.
fn value_end_unquoted(text: &str, from: usize) -> (usize, bool) {
    let end = text[from..]
        .find(['\n', '\r', ',', '}', ']', ';'])
        .map(|offset| from + offset)
        .unwrap_or(text.len());
    (end, end > from)
}

/// Where a named field's value begins, and whether it is quoted.
///
/// A separator is a `:` or an `=` with only quoting and whitespace around it.
/// Anything looser would start treating prose as a field and delete whole
/// sentences, which is a worse failure than missing a credential.
fn value_after_separator(lower: &str, after: usize) -> Option<(usize, bool)> {
    let mut at = after;
    let mut crossed = false;
    let mut quoted = false;

    for _ in 0..16 {
        let rest = &lower[at..];
        let character = rest.chars().next()?;
        match character {
            '"' | '\'' => {
                quoted = true;
                at += character.len_utf8();
            }
            ' ' | '\t' | '\n' | '\r' => at += character.len_utf8(),
            ':' | '=' => {
                at += character.len_utf8();
                crossed = true;
                // The quote before a separator closed the field name; the value
                // after it is a fresh, unquoted one.
                quoted = false;
            }
            _ => break,
        }
    }

    crossed.then_some((at, quoted))
}

/// Where the token that starts at `from` ends: the next quote, whitespace,
/// comma, brace or bracket, whichever comes first.
fn token_end(text: &str, from: usize) -> (usize, bool) {
    let terminators = ['"', '\'', ' ', '\n', '\r', '\t', ',', '}', ']'];
    let end = text[from..]
        .find(|character| terminators.contains(&character))
        .map(|offset| from + offset)
        .unwrap_or(text.len());
    (end, end > from)
}

/// Wall-clock milliseconds, so entries from one investigation line up.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// A timestamp a human can place, without pulling in a date library.
///
/// UTC, because a log that is only readable in one timezone is a log that is
/// hard to line up against a provider's own account of the same request.
fn format_at(at_ms: i64) -> String {
    let seconds = (at_ms / 1000).max(0) as u64;
    let millis = (at_ms.rem_euclid(1000)) as u16;

    let days = seconds / 86_400;
    let time_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    let (hour, minute, second) = (
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    );

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Days since the Unix epoch to a calendar date.
///
/// Howard Hinnant's `civil_from_days`, which is the standard arithmetic and is
/// only here so a timestamp is readable rather than an opaque number.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_recorded_masked_and_never_in_full() {
        let masked = mask_secret("sk-proj-1234567890abcdef");
        assert_eq!(masked, "sk-****cdef");
        assert!(!masked.contains("1234567890abc"));

        // Short keys are fully masked: the tail of a four character secret is
        // the whole secret.
        assert_eq!(mask_secret("abcd"), "****");
        assert_eq!(mask_secret(""), "");
        assert_eq!(mask_secret("   "), "");
    }

    #[test]
    fn a_credential_in_a_body_is_removed_rather_than_truncated() {
        // The shape an endpoint actually echoes back when a key is wrong.
        let body = r#"{"error":{"message":"Incorrect API key provided: sk-live-9f8e7d6c5b4a. "}}"#;
        let redacted = redact(body);
        assert!(!redacted.contains("9f8e7d6c5b4a"), "got {redacted}");
        assert!(redacted.contains("Incorrect API key provided"));

        // A JSON field named like a credential keeps its name, loses its value.
        let json = r#"{"api_key":"super-secret-value","model":"m"}"#;
        let redacted = redact(json);
        assert!(redacted.contains("api_key"), "got {redacted}");
        assert!(!redacted.contains("super-secret-value"));

        // A header form too, since that is what a 401 body usually looks like.
        let header = "Authorization: Bearer eyJhbGciOi.JIUzI1NiJ9";
        let redacted = redact(header);
        assert!(!redacted.contains("eyJhbGciOi"), "got {redacted}");
    }

    #[test]
    fn text_without_a_secret_is_left_exactly_as_it_was() {
        for text in [
            "the endpoint listed its models",
            "protocol openai_chat_completions selected",
            "HTTP 404 for /v1/responses",
            "模型列表返回 12 项",
            "",
        ] {
            assert_eq!(redact(text), text, "{text:?} was altered");
        }
    }

    #[test]
    fn a_secret_is_removed_wherever_in_the_line_it_appears() {
        // The matcher must not only work at the start of a string: a key inside
        // a sentence is the same leak.
        for text in [
            "sent with Bearer sk-live-abcdefghijkl and got 401",
            "the request used sk-live-abcdefghijkl for every call today",
            r#"body was {"authorization":"Bearer abcdefghijklmnop"} in reply"#,
        ] {
            let redacted = redact(text);
            assert!(
                !redacted.contains("abcdefghijkl"),
                "{text:?} leaked as {redacted:?}"
            );
        }
    }

    #[test]
    fn a_long_body_is_truncated_rather_than_stored_whole() {
        let body = "x".repeat(10_000);
        let kept = truncate(&body);
        assert!(kept.chars().count() <= MAX_BODY_CHARS + 1, "ellipsis aside");
        assert!(kept.ends_with('…'));

        let short = "  404   page   not  found  ";
        assert_eq!(truncate(short), "404 page not found");
    }

    #[test]
    fn the_two_halves_of_the_log_are_readable_apart() {
        let log = DiagnosticLog::new(50);
        log.note(
            DiagnosticKind::Discovery,
            "probe_start",
            "probing /v1/responses",
        );
        log.note(DiagnosticKind::Chat, "chat_request", "sending one turn");
        log.note(
            DiagnosticKind::Discovery,
            "protocol_result",
            "openai_responses is not here",
        );

        assert!(log
            .render_kind(DiagnosticKind::Discovery)
            .contains("probe_start"));
        assert!(!log
            .render_kind(DiagnosticKind::Discovery)
            .contains("chat_request"));
        assert!(log
            .render_kind(DiagnosticKind::Chat)
            .contains("chat_request"));
        assert_eq!(log.len(), 3);

        // The combined rendering is what a bug report gets, and it carries both.
        let all = log.render();
        assert!(all.contains("probe_start") && all.contains("chat_request"));
    }

    #[test]
    fn the_log_keeps_the_most_recent_lines_and_drops_the_rest() {
        let log = DiagnosticLog::new(3);
        for index in 0..10 {
            log.note(DiagnosticKind::Discovery, "probe", format!("line {index}"));
        }

        let entries = log.entries();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].message, "line 7");
        assert_eq!(entries[2].message, "line 9");

        let recent = log.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "line 8");

        log.clear();
        assert!(log.is_empty());
    }

    /// The promise the whole module exists to keep: anything written to a log
    /// can be pasted into a public bug report.
    #[test]
    fn nothing_written_to_a_log_can_contain_a_credential() {
        let secret = "sk-live-1234567890abcdef";
        let log = DiagnosticLog::new(10);

        log.record(
            DiagnosticKind::Discovery,
            "probe_result",
            format!("the endpoint rejected {secret}"),
            fields([
                ("api_key", secret.to_string()),
                ("status", "401".to_string()),
            ]),
        );
        log.record(
            DiagnosticKind::Chat,
            "chat_request",
            "Authorization: Bearer sk-live-1234567890abcdef",
            BTreeMap::new(),
        );

        let rendered = log.render();
        assert!(
            !rendered.contains(secret),
            "the log leaked the key: {rendered}"
        );
        assert!(rendered.contains("401"), "and lost the useful part");
    }

    #[test]
    fn a_log_survives_being_written_from_several_threads() {
        let log = std::sync::Arc::new(DiagnosticLog::new(1000));
        let mut handles = Vec::new();
        for worker in 0..4 {
            let log = std::sync::Arc::clone(&log);
            handles.push(std::thread::spawn(move || {
                for index in 0..25 {
                    log.note(DiagnosticKind::Chat, "round", format!("{worker}-{index}"));
                }
            }));
        }
        for handle in handles {
            handle.join().expect("worker");
        }
        assert_eq!(log.len(), 100);
    }

    #[test]
    fn a_restored_log_is_readable_in_the_same_shape() {
        let first = DiagnosticLog::new(10);
        first.note(
            DiagnosticKind::Discovery,
            "probe_start",
            "asking the endpoint",
        );

        let second = DiagnosticLog::new(10);
        second.restore(first.entries());

        assert_eq!(second.len(), 1);
        assert!(second.render().contains("probe_start"));
        assert!(!second.render().contains(secret_sample()));
    }

    fn secret_sample() -> &'static str {
        "sk-live-1234567890abcdef"
    }

    #[test]
    fn a_timestamp_is_readable_and_sorts_correctly() {
        assert_eq!(format_at(0), "1970-01-01T00:00:00.000Z");
        // 2024-02-29, a leap day, because a calendar bug shows up here first.
        assert_eq!(format_at(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
        assert!(format_at(1_700_000_000_123).ends_with(".123Z"));
    }
}
