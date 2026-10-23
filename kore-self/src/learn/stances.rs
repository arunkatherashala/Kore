//! Stance cues: explicit first-person phrases that announce a changed view.
//!
//! Recognised (case-insensitive, per sentence):
//!   "I now prefer X [over Y] [for TOPIC]"
//!   "I now think|believe CLAUSE"
//!   "I changed my mind about TOPIC[: detail]"   ("I've"/"I have" also)
//!   "from now on I will CLAUSE"
//! The phrase must start at a word boundary and must not follow "if", "when",
//! "whether", "imagine", "pretend" or "suppose". Quoted or reported speech
//! ("they now prefer tea") is not matched because the subject must be "I".
//! Each match is a candidate belief revision with a fixed confidence
//! (`STANCE_CONF`, not calibrated); nothing is inferred beyond the phrase.

use super::Ev;

pub const STANCE_CONF: f64 = 0.6;
const GUARDS: [&str; 6] = ["if", "when", "whether", "imagine", "pretend", "suppose"];
const MAX_TOPIC: usize = 60;

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub event_id: String,
    pub ts: String,
    /// prefer | think | mind | commit
    pub cue: &'static str,
    pub topic: String,
    pub stance: String,
    /// The matched sentence, normalised.
    pub phrase: String,
}

fn normalise(s: &str) -> String {
    let s = s.replace(['\u{2019}', '\u{2018}'], "'").to_lowercase();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_topic(s: &str) -> String {
    let t = s.trim().trim_matches(|c: char| !c.is_alphanumeric()).trim().to_string();
    if t.len() > MAX_TOPIC {
        let mut cut = MAX_TOPIC;
        while !t.is_char_boundary(cut) {
            cut -= 1;
        }
        t[..cut].trim().to_string()
    } else {
        t
    }
}

fn words_before(s: &str, pos: usize) -> Option<&str> {
    s[..pos].split_whitespace().last()
}

/// Find `needle` at a word boundary, not preceded by a guard word.
fn find_cue(sentence: &str, needle: &str) -> Option<usize> {
    for (pos, _) in sentence.match_indices(needle) {
        let boundary = pos == 0 || !sentence[..pos].chars().last().map_or(false, |c| c.is_alphanumeric());
        if !boundary {
            continue;
        }
        if let Some(w) = words_before(sentence, pos) {
            if GUARDS.contains(&w.trim_matches(|c: char| !c.is_alphanumeric())) {
                continue;
            }
        }
        return Some(pos);
    }
    None
}

fn parse_sentence(s: &str) -> Option<(&'static str, String, String)> {
    if let Some(p) = find_cue(s, "i now prefer ") {
        let rest = s[p + "i now prefer ".len()..].trim();
        let (obj, topic) = match rest.split_once(" for ") {
            Some((o, t)) => (o.trim(), t.trim()),
            None => (rest, rest.split(" over ").next().unwrap_or(rest)),
        };
        let (obj, topic) = (clean_topic(obj), clean_topic(topic));
        if obj.is_empty() || topic.is_empty() {
            return None;
        }
        return Some(("prefer", topic, format!("now prefers {obj}")));
    }
    for (needle, verb) in [("i now think ", "thinks"), ("i now believe ", "believes")] {
        if let Some(p) = find_cue(s, needle) {
            let clause = s[p + needle.len()..].trim();
            let clause = clause.strip_prefix("that ").unwrap_or(clause).trim();
            if clause.is_empty() {
                return None;
            }
            let topic = [" is ", " are ", " was ", " should ", " will "]
                .iter()
                .filter_map(|sep| clause.find(sep))
                .min()
                .map(|i| &clause[..i])
                .unwrap_or(clause);
            let topic = clean_topic(topic);
            if topic.is_empty() {
                return None;
            }
            return Some(("think", topic, format!("now {verb} that {}", clean_topic(clause))));
        }
    }
    for needle in ["i changed my mind about ", "i've changed my mind about ", "i have changed my mind about "] {
        if let Some(p) = find_cue(s, needle) {
            let rest = s[p + needle.len()..].trim();
            let end = rest.find([':', ',']).unwrap_or(rest.len());
            let topic = clean_topic(&rest[..end]);
            if topic.is_empty() {
                return None;
            }
            let detail = rest[end..].trim_start_matches([':', ',', ' ']).trim();
            let stance = if detail.is_empty() { format!("changed mind about {topic}") } else { format!("changed mind about {topic}: {detail}") };
            return Some(("mind", topic, stance));
        }
    }
    for needle in ["from now on i will ", "from now on, i will "] {
        if let Some(p) = find_cue(s, needle) {
            let clause = clean_topic(&s[p + needle.len()..]);
            if clause.is_empty() {
                return None;
            }
            let topic: String = clause.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
            return Some(("commit", topic, format!("commits from now on to {clause}")));
        }
    }
    None
}

/// Extract at most one cue per sentence from every event, in event order.
pub fn extract(events: &[Ev]) -> Vec<Cue> {
    let mut out = Vec::new();
    for e in events {
        for sentence in e.text.split(['.', '!', '?', ';', '\n']) {
            let s = normalise(sentence);
            if s.is_empty() {
                continue;
            }
            if let Some((cue, topic, stance)) = parse_sentence(&s) {
                out.push(Cue { event_id: e.id.clone(), ts: e.ts.clone(), cue, topic, stance, phrase: s });
            }
        }
    }
    out
}
