//! Keyword rules that give free-text events virtual tags at read time.
//!
//! The owner writes `<owner dir>/tag_rules.txt` by hand (no tool can read or
//! write it; see `safety::safe_path`). Lines:
//!
//! ```text
//! # comment
//! topic:cooking = cooking, recipe, baking      # a mention of the topic
//! action:gym    = gym, workout, lifted         # an action that can be taken
//! ctx:morning   = morning                      # a context that holds
//! negation      = actions                      # actions (default) | all | off
//! builtin_contexts = on                        # on (default) | off
//! ```
//!
//! Matching is case-insensitive and whole-word (hand-written tokenizer, no
//! regex): letters and digits of any script form words, an apostrophe between
//! letters stays inside a word, a trailing `'s` is ignored, a keyword may be
//! several words ("train ride") and must match consecutive words.
//!
//! Negation guard (best effort): a keyword hit does not count when one of the
//! 4 words before it, in the same sentence, is a negator (not, no, never,
//! without, skipped, missed, cancelled, avoided, ...n't). It catches "did not
//! go to the gym" and "skipped gym"; it misses "the gym was closed" and
//! mis-fires on "not quit going to gym". It applies to `action:` rules
//! unless `negation` says otherwise.
//!
//! How habits count occasions (see `habits.rs`): a keyword context `ctx:C` is
//! an occasion for every event with a sentence matching C; the action was
//! taken if an `action:` rule matches in a sentence that also matches C (same
//! sentence: "Morning: went to the gym."). Built-in date contexts are
//! day-level: every day with at least one event; the action was taken if any
//! event that day matches the action's rule.
//!
//! Built-in contexts (no keywords): `ctx:weekday` and `ctx:weekend`, from the
//! UTC date of `occurred_at`. They are active when at least one `action:` rule
//! exists, unless `builtin_contexts = off`.
//!
//! Rules are applied when learning reads the log; stored events and stored
//! tags are never rewritten, so editing the file and re-running learning
//! re-derives everything (rows whose rationale carries the rules fingerprint
//! change when the rules change). std + `crate::versioned` only.

use super::Ev;
use crate::versioned::sha256_hex;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

pub const RULES_FILE: &str = "tag_rules.txt";
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_RULES: usize = 200;
pub const MAX_KEYWORDS: usize = 100;
pub const MAX_KEYWORD_BYTES: usize = 64;
pub const NEG_WINDOW: usize = 4;
pub const WEEKDAY: &str = "weekday";
pub const WEEKEND: &str = "weekend";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Topic,
    Action,
    Ctx,
}

impl RuleKind {
    pub fn prefix(self) -> &'static str {
        match self {
            RuleKind::Topic => "topic",
            RuleKind::Action => "action",
            RuleKind::Ctx => "ctx",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegMode {
    Actions,
    All,
    Off,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub kind: RuleKind,
    pub name: String,
    /// Keywords as written (lowercased, trimmed), for display.
    pub keywords: Vec<String>,
    toks: Vec<Vec<String>>,
}

impl Rule {
    pub fn label(&self) -> String {
        format!("{}:{}", self.kind.prefix(), self.name)
    }
}

/// Per-text classification cache (backtests classify the same events many
/// times). Ignored by equality, empty after `clone`.
#[derive(Default)]
struct Cache(RefCell<HashMap<String, Vec<Classified>>>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        Cache::default()
    }
}
impl PartialEq for Cache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cache")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TagRules {
    pub rules: Vec<Rule>,
    pub neg: NegMode,
    pub builtin: bool,
    pub warnings: Vec<String>,
    /// First 12 hex of sha256 over the canonical rules text.
    pub fingerprint: String,
    cache: Cache,
}

impl Default for TagRules {
    fn default() -> Self {
        TagRules::parse("")
    }
}

/// What the rules say about one event's text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Classified {
    pub topics: Vec<String>,
    pub actions: Vec<String>,
    pub ctxs: Vec<String>,
}

impl Classified {
    pub fn is_empty(&self) -> bool {
        self.topics.is_empty() && self.actions.is_empty() && self.ctxs.is_empty()
    }
}

/// Per-rule outcome on one text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub rule: usize,
    /// Non-negated keyword occurrences.
    pub positive: usize,
    /// Occurrences suppressed by the negation guard.
    pub negated: usize,
}

struct Tok {
    w: String,
    neg: bool,
    sent: u32,
}

const NEGATORS: [&str; 24] = [
    "not", "no", "never", "without", "skipped", "skip", "skipping", "missed", "miss", "cancelled", "canceled", "avoided", "nothing", "neither", "nor", "didnt", "dont", "wasnt", "cant", "couldnt", "wont", "havent", "hadnt", "isnt",
];

fn is_negator(w: &str) -> bool {
    w.ends_with("n't") || NEGATORS.contains(&w)
}

fn push_tok(out: &mut Vec<Tok>, cur: &mut String, sent: u32) {
    if cur.is_empty() {
        return;
    }
    let word = std::mem::take(cur);
    let neg = is_negator(&word);
    let w = word.strip_suffix("'s").filter(|s| !s.is_empty()).map(str::to_string).unwrap_or(word);
    out.push(Tok { w, neg, sent });
}

fn tokenize(s: &str) -> Vec<Tok> {
    let chars: Vec<char> = s.chars().collect();
    let (mut out, mut cur, mut sent) = (Vec::new(), String::new(), 0u32);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            cur.extend(c.to_lowercase());
        } else if (c == '\'' || c == '\u{2019}') && !cur.is_empty() && chars.get(i + 1).map_or(false, |n| n.is_alphanumeric()) {
            cur.push('\'');
        } else {
            push_tok(&mut out, &mut cur, sent);
            if matches!(c, '.' | '!' | '?' | ';' | '\n' | '\r') {
                sent += 1;
            }
        }
    }
    push_tok(&mut out, &mut cur, sent);
    out
}

fn words_of(s: &str) -> Vec<String> {
    tokenize(s).into_iter().map(|t| t.w).collect()
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') && n.chars().count() <= 40
}

impl TagRules {
    /// Parse the file text. Bad lines are skipped and reported in `warnings`.
    pub fn parse(text: &str) -> TagRules {
        let mut r = TagRules { rules: Vec::new(), neg: NegMode::Actions, builtin: true, warnings: Vec::new(), fingerprint: String::new(), cache: Cache::default() };
        for (ln, raw) in text.lines().enumerate() {
            let line = match raw.find('#') {
                Some(i) => &raw[..i],
                None => raw,
            }
            .trim();
            if line.is_empty() {
                continue;
            }
            let at = ln + 1;
            let Some((lhs, rhs)) = line.split_once('=') else {
                r.warnings.push(format!("line {at} ignored (no '='): {}", crate::learn::clip(line, 60)));
                continue;
            };
            let (lhs, rhs) = (lhs.trim().to_lowercase(), rhs.trim());
            match lhs.as_str() {
                "negation" => {
                    match rhs.to_lowercase().as_str() {
                        "actions" => r.neg = NegMode::Actions,
                        "all" => r.neg = NegMode::All,
                        "off" => r.neg = NegMode::Off,
                        _ => r.warnings.push(format!("line {at}: negation must be actions, all or off")),
                    }
                    continue;
                }
                "builtin_contexts" => {
                    match rhs.to_lowercase().as_str() {
                        "on" => r.builtin = true,
                        "off" => r.builtin = false,
                        _ => r.warnings.push(format!("line {at}: builtin_contexts must be on or off")),
                    }
                    continue;
                }
                _ => {}
            }
            let Some((k, name)) = lhs.split_once(':') else {
                r.warnings.push(format!("line {at} ignored (name must look like topic:x, action:x or ctx:x)"));
                continue;
            };
            let kind = match k.trim() {
                "topic" => RuleKind::Topic,
                "action" => RuleKind::Action,
                "ctx" => RuleKind::Ctx,
                _ => {
                    r.warnings.push(format!("line {at} ignored (unknown kind '{}'; use topic, action or ctx)", k.trim()));
                    continue;
                }
            };
            let name = name.trim().to_string();
            if !valid_name(&name) {
                r.warnings.push(format!("line {at} ignored (bad name '{}': letters, digits, - and _ only)", crate::learn::clip(&name, 40)));
                continue;
            }
            if kind == RuleKind::Ctx && (name == WEEKDAY || name == WEEKEND) {
                r.warnings.push(format!("line {at} ignored: ctx:{name} is built in (from the event date) and takes no keywords"));
                continue;
            }
            let mut kws: Vec<String> = Vec::new();
            let mut toks: Vec<Vec<String>> = Vec::new();
            for kw in rhs.split(',') {
                let kw = kw.trim().trim_matches('"').trim().to_lowercase();
                if kw.is_empty() {
                    continue;
                }
                if kw.len() > MAX_KEYWORD_BYTES {
                    r.warnings.push(format!("line {at}: keyword longer than {MAX_KEYWORD_BYTES} bytes ignored"));
                    continue;
                }
                let t = words_of(&kw);
                if t.is_empty() {
                    r.warnings.push(format!("line {at}: keyword '{kw}' has no letters or digits, ignored"));
                    continue;
                }
                kws.push(kw);
                toks.push(t);
            }
            if toks.is_empty() {
                r.warnings.push(format!("line {at} ignored (no usable keywords)"));
                continue;
            }
            if let Some(ex) = r.rules.iter_mut().find(|x| x.kind == kind && x.name == name) {
                ex.keywords.extend(kws);
                ex.toks.extend(toks);
                if ex.toks.len() > MAX_KEYWORDS {
                    ex.keywords.truncate(MAX_KEYWORDS);
                    ex.toks.truncate(MAX_KEYWORDS);
                    r.warnings.push(format!("line {at}: more than {MAX_KEYWORDS} keywords for {}:{name}, extra ignored", kind.prefix()));
                }
                continue;
            }
            if r.rules.len() >= MAX_RULES {
                r.warnings.push(format!("line {at} ignored: more than {MAX_RULES} rules"));
                continue;
            }
            if toks.len() > MAX_KEYWORDS {
                kws.truncate(MAX_KEYWORDS);
                toks.truncate(MAX_KEYWORDS);
                r.warnings.push(format!("line {at}: more than {MAX_KEYWORDS} keywords, extra ignored"));
            }
            r.rules.push(Rule { kind, name, keywords: kws, toks });
        }
        let mut canon: Vec<String> = r.rules.iter().map(|x| format!("{}={}", x.label(), x.keywords.join(","))).collect();
        canon.sort();
        canon.push(format!("negation={:?};builtin={}", r.neg, r.builtin));
        let h = sha256_hex(&[canon.join("\n").as_bytes()]);
        r.fingerprint = h.chars().take(12).collect();
        r
    }

    /// Read `<owner_dir>/tag_rules.txt`. None when the file does not exist.
    /// An unreadable or oversized file yields an empty rule set with a warning.
    pub fn load(owner_dir: &Path) -> Option<TagRules> {
        let p = owner_dir.join(RULES_FILE);
        let meta = std::fs::metadata(&p).ok()?;
        if !meta.is_file() {
            return None;
        }
        if meta.len() > MAX_FILE_BYTES {
            let mut r = TagRules::default();
            r.warnings.push(format!("{RULES_FILE} is larger than {MAX_FILE_BYTES} bytes and was ignored"));
            return Some(r);
        }
        match std::fs::read(&p) {
            Ok(b) => Some(TagRules::parse(&String::from_utf8_lossy(&b))),
            Err(e) => {
                let mut r = TagRules::default();
                r.warnings.push(format!("{RULES_FILE} could not be read: {e}"));
                Some(r)
            }
        }
    }

    pub fn has_actions(&self) -> bool {
        self.rules.iter().any(|r| r.kind == RuleKind::Action)
    }
    pub fn has_topics(&self) -> bool {
        self.rules.iter().any(|r| r.kind == RuleKind::Topic)
    }

    fn guarded(&self, k: RuleKind) -> bool {
        match self.neg {
            NegMode::Off => false,
            NegMode::All => true,
            NegMode::Actions => k == RuleKind::Action,
        }
    }

    fn scan(&self, rule: &Rule, toks: &[Tok]) -> (usize, usize) {
        let guard = self.guarded(rule.kind);
        let (mut pos, mut neg) = (0, 0);
        for i in 0..toks.len() {
            for kw in &rule.toks {
                if i + kw.len() <= toks.len() && kw.iter().zip(&toks[i..]).all(|(a, b)| *a == b.w) {
                    let negated = guard && toks[i.saturating_sub(NEG_WINDOW)..i].iter().any(|t| t.neg && t.sent == toks[i].sent);
                    if negated {
                        neg += 1;
                    } else {
                        pos += 1;
                    }
                }
            }
        }
        (pos, neg)
    }

    /// Every rule that has at least one keyword occurrence (counted or suppressed).
    pub fn hits(&self, text: &str) -> Vec<Hit> {
        let toks = tokenize(text);
        self.rules
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let (positive, negated) = self.scan(r, &toks);
                (positive + negated > 0).then_some(Hit { rule: i, positive, negated })
            })
            .collect()
    }

    /// One `Classified` per sentence of the text that has any match (sentences
    /// end at . ! ? ; or a line break; ':' and ',' do not end one). Negation
    /// guard as in `classify`.
    pub fn classify_sentences(&self, text: &str) -> Vec<Classified> {
        if let Some(c) = self.cache.0.borrow().get(text) {
            return c.clone();
        }
        let v = self.classify_sentences_uncached(text);
        let mut m = self.cache.0.borrow_mut();
        if m.len() > 200_000 {
            m.clear();
        }
        m.insert(text.to_string(), v.clone());
        v
    }

    fn classify_sentences_uncached(&self, text: &str) -> Vec<Classified> {
        let toks = tokenize(text);
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let mut j = i;
            while j < toks.len() && toks[j].sent == toks[i].sent {
                j += 1;
            }
            let seg = &toks[i..j];
            let mut c = Classified::default();
            let (mut t, mut a, mut x) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
            for r in &self.rules {
                if self.scan(r, seg).0 > 0 {
                    match r.kind {
                        RuleKind::Topic => t.insert(r.name.clone()),
                        RuleKind::Action => a.insert(r.name.clone()),
                        RuleKind::Ctx => x.insert(r.name.clone()),
                    };
                }
            }
            c.topics = t.into_iter().collect();
            c.actions = a.into_iter().collect();
            c.ctxs = x.into_iter().collect();
            if !c.is_empty() {
                out.push(c);
            }
            i = j;
        }
        out
    }

    /// Names (deduplicated, sorted) of the rules that hold for this text.
    /// Built-in date contexts are not included here.
    pub fn classify(&self, text: &str) -> Classified {
        let (mut t, mut a, mut x) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for c in self.classify_sentences(text) {
            t.extend(c.topics);
            a.extend(c.actions);
            x.extend(c.ctxs);
        }
        Classified { topics: t.into_iter().collect(), actions: a.into_iter().collect(), ctxs: x.into_iter().collect() }
    }

    /// Copies of `events` with a virtual `topic:<T>` tag added where a topic
    /// rule matches and the event does not already carry that tag. Also
    /// returns the topics that received at least one virtual tag.
    pub fn annotate_topics(&self, events: &[Ev]) -> (Vec<Ev>, BTreeSet<String>) {
        let mut virt = BTreeSet::new();
        if !self.has_topics() {
            return (events.to_vec(), virt);
        }
        let out = events
            .iter()
            .map(|e| {
                let mut e = e.clone();
                for t in self.classify(&e.text).topics {
                    let tag = format!("{}{}", super::TOPIC_PREFIX, t);
                    if !e.has_tag(&tag) {
                        e.tags.push(tag);
                        virt.insert(t);
                    }
                }
                e
            })
            .collect();
        (out, virt)
    }
}

/// Monday = 0 .. Sunday = 6 for a day number (day 0 = Thursday 1970-01-01).
pub fn weekday_index(day: i64) -> i64 {
    (day + 3).rem_euclid(7)
}

pub fn builtin_context(day: i64) -> &'static str {
    if weekday_index(day) < 5 {
        WEEKDAY
    } else {
        WEEKEND
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> TagRules {
        TagRules::parse(s)
    }

    #[test]
    fn whole_word_case_insensitive_unicode() {
        let t = r("topic:gym = gym, café au lait\naction:run = run");
        assert_eq!(t.classify("GYM today").topics, vec!["gym"]);
        assert!(t.classify("gymnastics and gymnasium").topics.is_empty());
        assert_eq!(t.classify("the gym's door").topics, vec!["gym"]);
        assert_eq!(t.classify("Had a CAFÉ au   lait.").topics, vec!["gym"]);
        assert!(t.classify("cafés au lait").topics.is_empty());
        assert_eq!(t.classify("go-run!").actions, vec!["run"]);
        assert!(t.classify("running ran").actions.is_empty());
        let u = r("topic:ünï = ünï\ntopic:jp = 日本");
        assert_eq!(u.classify("Ünï!").topics, vec!["ünï"]);
        assert_eq!(u.classify("日本 trip").topics, vec!["jp"]);
    }

    #[test]
    fn negation_guard_is_best_effort() {
        let t = r("action:gym = gym");
        for yes in ["went to the gym", "Gym then work. Not tired.", "excuses, gym again", "I did go to the gym"] {
            assert_eq!(t.classify(yes).actions, vec!["gym"], "{yes}");
        }
        for no in ["did not go to the gym", "skipped gym today", "I didn't go to the gym", "didn\u{2019}t make the gym", "never at the gym", "went out without the gym"] {
            assert!(t.classify(no).actions.is_empty(), "{no}");
            assert!(t.hits(no)[0].negated > 0);
        }
        // documented misses and mis-fires
        assert_eq!(t.classify("the gym was closed").actions, vec!["gym"]);
        assert!(t.classify("not quit going to gym").actions.is_empty());
        // too far from the negator, or in another sentence
        assert_eq!(t.classify("not sure what else to say about my week but gym").actions, vec!["gym"]);
        assert_eq!(t.classify("Did not sleep. Gym later").actions, vec!["gym"]);
        // topics are not guarded by default, but negation=all guards them
        let tt = r("topic:cooking = cooking");
        assert_eq!(tt.classify("not into cooking").topics, vec!["cooking"]);
        assert!(r("negation = all\ntopic:cooking = cooking").classify("not into cooking").topics.is_empty());
        assert_eq!(r("negation = off\naction:gym = gym").classify("skipped gym").actions, vec!["gym"]);
    }

    #[test]
    fn parse_reports_bad_lines_and_builtin_names() {
        let t = r("# c\nfoo\ntopic:a b = x\nweird:x = y\nctx:weekday = monday\ntopic:ok = \naction:gym = gym # trailing\nnegation = maybe\n");
        assert_eq!(t.rules.len(), 1);
        assert_eq!(t.warnings.len(), 6, "{:?}", t.warnings);
        assert_eq!(t.rules[0].label(), "action:gym");
        assert_ne!(r("topic:a = x").fingerprint, r("topic:a = y").fingerprint);
        assert_eq!(r("topic:a = x\n").fingerprint, r("# hi\ntopic:a = x # c\n").fingerprint);
        // repeated names merge
        let m = r("topic:a = x\ntopic:a = y");
        assert_eq!(m.rules.len(), 1);
        assert_eq!(m.rules[0].keywords, vec!["x", "y"]);
    }

    #[test]
    fn weekdays_from_day_numbers() {
        // 1970-01-01 Thursday; 2026-07-01 was a Wednesday.
        assert_eq!(weekday_index(0), 3);
        assert_eq!(weekday_index(super::super::dates::days_from_civil(2026, 7, 1)), 2);
        assert_eq!(builtin_context(super::super::dates::days_from_civil(2026, 7, 4)), WEEKEND);
        assert_eq!(builtin_context(super::super::dates::days_from_civil(2026, 7, 6)), WEEKDAY);
        assert_eq!(weekday_index(-1), 2);
    }
}
