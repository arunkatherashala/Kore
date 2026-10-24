//! Synthetic life generator with ground truth (tests and measurement only).
//!
//! A seeded in-crate PRNG (splitmix64) produces N days of dated events from a
//! configurable truth: planted habits (action X in context C with probability
//! p), planted topic change points (daily mention rate shifts at day D),
//! planted stance statements (explicit phrases), plus noise events. The same
//! seed and config give byte-identical events. The ground truth is returned
//! next to the events so detectors can be scored.
//!
//! The data is Poisson/uniform by construction, so numbers measured on it say
//! how the detectors behave on that kind of data, not on a real life.
#![allow(dead_code)]

use crate::learn::{dates, Ev};
use crate::versioned::{event_id, normalize_ts, NewEvent};

/// splitmix64.
#[derive(Debug, Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Uniform in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    /// Poisson via Knuth's product method (fine for small lambda).
    pub fn poisson(&mut self, lambda: f64) -> u32 {
        let l = (-lambda).exp();
        let (mut k, mut p) = (0u32, 1.0f64);
        loop {
            p *= self.f64();
            if p <= l {
                return k;
            }
            k += 1;
            if k > 1000 {
                return k;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct HabitSpec {
    pub context: String,
    pub action: String,
    pub p: f64,
}

#[derive(Debug, Clone)]
pub struct TopicSpec {
    pub topic: String,
    pub base_rate: f64,
    /// (day offset, new daily rate), ascending.
    pub shifts: Vec<(i64, f64)>,
}

#[derive(Debug, Clone)]
pub struct StanceSpec {
    pub day: i64,
    pub topic: String,
    /// 0 prefer, 1 think, 2 mind, 3 commit
    pub template: u8,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct SynthConfig {
    pub seed: u64,
    pub start_day: i64,
    pub days: i64,
    /// Expected occasions per day for each context.
    pub occasions_per_day: f64,
    pub actions: Vec<String>,
    pub habits: Vec<HabitSpec>,
    /// Contexts whose action is uniform over `actions`.
    pub null_contexts: Vec<String>,
    /// (context, action, occasions): fewer occasions than any sensible min n.
    pub rare_contexts: Vec<(String, String, u32)>,
    pub topics: Vec<TopicSpec>,
    pub stances: Vec<StanceSpec>,
    /// Noise events added, as a fraction of signal events (0.0, 0.2, 0.5, ...).
    pub noise: f64,
    /// Free-text mode: events carry no tags and read like short journal sentences
    /// (see `free_text_rules`). Default false: output is unchanged.
    pub free_text: bool,
}

/// A planted habit counts as a true habit at or above this probability.
pub const TRUE_HABIT_P: f64 = 0.6;

#[derive(Debug, Clone)]
pub struct TruthChange {
    pub topic: String,
    pub day: i64,
    pub from: f64,
    pub to: f64,
}

#[derive(Debug, Clone)]
pub struct TruthStance {
    pub event_id: String,
    pub day: i64,
    pub topic: String,
}

#[derive(Debug, Clone, Default)]
pub struct Truth {
    /// (context, action, p) with p >= TRUE_HABIT_P.
    pub habits: Vec<(String, String, f64)>,
    /// Planted contexts that are not habits (weak p, or uniform).
    pub non_habit_contexts: Vec<String>,
    pub rare_contexts: Vec<String>,
    pub changes: Vec<TruthChange>,
    pub steady_topics: Vec<String>,
    pub stances: Vec<TruthStance>,
    pub start_day: i64,
    pub days: i64,
}

pub fn default_config(seed: u64, noise: f64) -> SynthConfig {
    let s = |x: &str| x.to_string();
    let actions: Vec<String> = ["run", "read", "podcast", "hike", "cook", "nap"].iter().map(|x| s(x)).collect();
    let steady = [("cooking", 1.0), ("music", 1.5), ("travel", 0.8), ("health", 1.2), ("books", 2.0)];
    let mut topics = vec![
        TopicSpec { topic: s("gardening"), base_rate: 0.4, shifts: vec![(60, 2.0)] },
        TopicSpec { topic: s("politics"), base_rate: 3.0, shifts: vec![(90, 0.8)] },
        TopicSpec { topic: s("startup"), base_rate: 0.5, shifts: vec![(100, 1.5)] },
    ];
    for (t, r) in steady {
        topics.push(TopicSpec { topic: s(t), base_rate: r, shifts: vec![] });
    }
    SynthConfig {
        seed,
        start_day: dates::days_from_civil(2024, 1, 1),
        days: 150,
        occasions_per_day: 0.8,
        actions,
        habits: vec![
            HabitSpec { context: s("morning"), action: s("run"), p: 0.9 },
            HabitSpec { context: s("evening"), action: s("read"), p: 0.8 },
            HabitSpec { context: s("commute"), action: s("podcast"), p: 0.7 },
            HabitSpec { context: s("afternoon"), action: s("cook"), p: 0.35 },
        ],
        null_contexts: vec![s("lunch"), s("night")],
        rare_contexts: vec![(s("retreat"), s("hike"), 4), (s("conference"), s("nap"), 3)],
        topics,
        stances: vec![
            StanceSpec { day: 20, topic: s("remote-work"), template: 0, detail: s("tea") },
            StanceSpec { day: 45, topic: s("meetings"), template: 1, detail: s("overrated") },
            StanceSpec { day: 70, topic: s("diet"), template: 2, detail: s("fasting works for me") },
            StanceSpec { day: 95, topic: s("reading"), template: 3, detail: s("finish") },
            StanceSpec { day: 120, topic: s("remote-work"), template: 1, detail: s("useful") },
            StanceSpec { day: 135, topic: s("cycling"), template: 0, detail: s("bikes") },
        ],
        noise,
        free_text: false,
    }
}

/// The default life written as free text instead of tags (tests and measurement).
pub fn free_text_config(seed: u64, noise: f64) -> SynthConfig {
    let mut c = default_config(seed, noise);
    c.free_text = true;
    c
}

fn stamp(day: i64, counter: u32) -> String {
    let secs = 6 * 3600 + counter * 37;
    format!("{}T{:02}:{:02}:{:02}Z", dates::date_of(day), secs / 3600, secs % 3600 / 60, secs % 60)
}

pub fn stance_text(s: &StanceSpec) -> (String, String) {
    match s.template {
        0 => (format!("I now prefer {} for {}.", s.detail, s.topic), s.topic.clone()),
        1 => (format!("I now think {} is {}.", s.topic, s.detail), s.topic.clone()),
        2 => (format!("I changed my mind about {}: {}.", s.topic, s.detail), s.topic.clone()),
        _ => {
            let clause = format!("{} {}", s.detail, s.topic);
            (format!("From now on I will {clause}."), clause.split_whitespace().take(6).collect::<Vec<_>>().join(" "))
        }
    }
}

const NEAR_MISS: [&str; 6] = [
    "They now prefer coffee for work.",
    "I prefer tea for mornings.",
    "If I now prefer tea for mornings that would be odd.",
    "She now thinks the plan is bad.",
    "I changed my plans about lunch.",
    "Had lunch and read the news.",
];

// ── free-text mode ──────────────────────────────────────────────────────────
// Sentence templates written by the same author as the keyword rules below.
// They contain varied phrasing, negations the guard can and cannot see,
// keyword-bearing distractors, and synonyms that the rules do NOT list (so
// some real occasions are invisible by construction).

const UNLISTED_SHARE: f64 = 0.10;

fn pick<'a>(rng: &mut Rng, v: &[&'a str]) -> &'a str {
    v[rng.below(v.len())]
}

fn ctx_phrase(rng: &mut Rng, ctx: &str) -> &'static str {
    match ctx {
        "morning" => pick(rng, &["This morning", "Morning:", "Early morning,", "First thing in the morning,"]),
        "evening" => pick(rng, &["In the evening", "Evening:", "Tonight", "After dinner this evening,"]),
        "commute" => pick(rng, &["On the commute", "Commute:", "Train ride in,", "During the commute"]),
        "afternoon" => pick(rng, &["This afternoon", "Afternoon:", "Mid afternoon,"]),
        "lunch" => pick(rng, &["At lunch", "Lunch:", "Over lunch"]),
        "night" => pick(rng, &["Late at night", "Night:", "Before bed at night,"]),
        "retreat" => pick(rng, &["At the retreat", "Retreat day:"]),
        "conference" => pick(rng, &["At the conference", "Conference day:"]),
        _ => "Today",
    }
}

fn action_clause(rng: &mut Rng, act: &str, unlisted: bool) -> &'static str {
    match (act, unlisted) {
        ("run", false) => pick(rng, &["went for a run", "ran 5k", "jogged around the park", "did a quick run before work"]),
        ("run", true) => "did laps around the track",
        ("read", false) => pick(rng, &["read a few chapters", "spent an hour reading", "finished a book I read slowly"]),
        ("read", true) => "got through a few chapters of the novel",
        ("podcast", false) => pick(rng, &["listened to a podcast", "caught up on a podcast episode", "two podcasts back to back"]),
        ("podcast", true) => "listened to an audio show",
        ("hike", false) => pick(rng, &["hiked up the ridge", "went on a long hike", "hiked the loop trail"]),
        ("hike", true) => "walked the mountain trail",
        ("cook", false) => pick(rng, &["cooked dinner for friends", "cook night, tried something new", "made dinner from scratch"]),
        ("cook", true) => "put together a big pot of stew",
        ("nap", false) => pick(rng, &["took a nap", "napped for an hour", "a short nap on the sofa"]),
        ("nap", true) => "dozed off on the sofa",
        _ => "did something",
    }
}

/// Negated mentions of an action. The last form of each is a post-negation
/// ("... was called off") that the guard does not see by design.
fn negated_clause(rng: &mut Rng, act: &str) -> &'static str {
    let (a, b, c, post) = match act {
        "run" => ("did not run at all", "skipped the run", "no run today", "the run was called off"),
        "read" => ("did not read anything", "skipped reading", "no reading at all", "the reading was put off"),
        "podcast" => ("no podcast today", "skipped the podcast", "never put a podcast on", "the podcast went unplayed"),
        "hike" => ("didn't go for a hike", "skipped the hike", "no hike this time", "the hike was cancelled by rain"),
        "cook" => ("did not cook", "skipped cook night", "no cook night, ordered in", "cook night got postponed"),
        "nap" => ("did not nap", "skipped the nap", "no nap, too busy", "the nap never happened"),
        _ => ("did not do it", "skipped it", "no luck", "it was off"),
    };
    // 15% post-negations
    if rng.f64() < 0.15 {
        post
    } else {
        pick(rng, &[a, b, c])
    }
}

/// One occasion as a sentence. `planted` is the habit's own action when the
/// occasion did not take it: 40% of those mention it negated instead of doing
/// something else.
fn free_action(rng: &mut Rng, ctx: &str, act: &str, planted: Option<&str>) -> String {
    let c = ctx_phrase(rng, ctx);
    if let Some(pl) = planted {
        if rng.f64() < 0.4 {
            return format!("{c} {}.", negated_clause(rng, pl));
        }
    }
    let unlisted = rng.f64() < UNLISTED_SHARE;
    format!("{c} {}.", action_clause(rng, act, unlisted))
}

fn free_topic(rng: &mut Rng, topic: &str, noise: bool) -> String {
    let unlisted = rng.f64() < UNLISTED_SHARE;
    let (listed, other): (&[&str], &str) = match topic {
        "gardening" => (&["Spent a while gardening", "Worked in the garden", "Planted tomatoes", "Gardening notes for next year"], "Pulled weeds and watered the beds"),
        "politics" => (&["Followed the election news", "Argued about politics again", "The senate vote kept me up", "Too much politics on the feed"], "Doomscrolled the headlines"),
        "startup" => (&["Worked on the startup idea", "Wrote the pitch deck", "Coffee with two founders", "Startup finances are a mess"], "Sketched the business plan"),
        "cooking" => (&["Tried a new recipe", "Baking day", "Cooking is a good break", "Copied a recipe into the notebook"], "Tested a sourdough starter"),
        "music" => (&["Practiced guitar", "Music playlist for the week", "Went to a concert", "Found new music"], "Learned a new chord shape"),
        "travel" => (&["Booked a flight", "Planning the trip", "Travel insurance paperwork", "Packing list for the trip"], "Checked passport dates"),
        "health" => (&["Doctor appointment", "Health insurance paperwork", "Annual checkup went fine", "Thinking about health"], "Blood test results came back"),
        "books" => (&["Borrowed books from the library", "Started a new novel", "Library hold arrived", "Sorted the shelf of old books"], "Rearranged the shelf of paperbacks"),
        _ => (&["Thought about it"], "Thought about it"),
    };
    let _ = noise;
    if unlisted {
        other.to_string()
    } else {
        format!("{}.", listed[rng.below(listed.len())])
    }
}

/// Chatter that carries action keywords without being the action.
const DISTRACTORS: [&str; 8] = [
    "The bus ran late again.",
    "My reading glasses broke.",
    "The podcast app crashed on my phone.",
    "Hike permit prices went up.",
    "The cook at the corner cafe was friendly.",
    "The baby's nap schedule is a mess.",
    "Updated the project plan and replied to emails.",
    "Watched the game with friends.",
];

/// The keyword rules that match the free-text templates above (what a user of
/// this life would plausibly write after looking at `self_tag_rules_check`).
pub fn free_text_rules() -> String {
    "# synthetic life: rules written by the same author as the templates\n\
     negation = actions\n\
     ctx:morning = morning\n\
     ctx:evening = evening, tonight\n\
     ctx:commute = commute, train ride\n\
     ctx:afternoon = afternoon\n\
     ctx:lunch = lunch\n\
     ctx:night = night\n\
     ctx:retreat = retreat\n\
     ctx:conference = conference\n\
     action:run = run, ran, jogged\n\
     action:read = read, reading\n\
     action:podcast = podcast, podcasts\n\
     action:hike = hike, hiked\n\
     action:cook = cook, cooked, made dinner\n\
     action:nap = nap, napped\n\
     topic:gardening = gardening, garden, tomatoes\n\
     topic:politics = politics, election, senate\n\
     topic:startup = startup, founders, pitch\n\
     topic:cooking = cooking, recipe, baking\n\
     topic:music = music, guitar, concert\n\
     topic:travel = travel, flight, trip\n\
     topic:health = health, doctor, checkup\n\
     topic:books = books, novel, library\n"
        .to_string()
}

pub fn generate(cfg: &SynthConfig) -> (Vec<NewEvent>, Truth) {
    let mut rng = Rng::new(cfg.seed);
    let mut per_day: std::collections::BTreeMap<i64, u32> = Default::default();
    let mut out: Vec<NewEvent> = Vec::new();
    let mut truth = Truth { start_day: cfg.start_day, days: cfg.days, ..Default::default() };
    let push = |out: &mut Vec<NewEvent>, per_day: &mut std::collections::BTreeMap<i64, u32>, day: i64, source: &str, kind: &str, text: String, tags: Vec<String>| -> String {
        let c = per_day.entry(day).or_insert(0);
        let ts = stamp(day, *c);
        *c += 1;
        let id = event_id(source, &normalize_ts(&ts).expect("valid"), &text);
        out.push(NewEvent { source: source.into(), occurred_at: ts, kind: kind.into(), text, tags, redacted: false });
        id
    };
    let action_event = |ctx: &str, act: &str| (format!("context {ctx}: did {act}"), vec![format!("ctx:{ctx}"), format!("act:{act}")]);
    let other = |rng: &mut Rng, not: &str, actions: &[String]| -> String {
        let c: Vec<&String> = actions.iter().filter(|a| a.as_str() != not).collect();
        c[rng.below(c.len())].clone()
    };

    // habits and uniform contexts
    for h in &cfg.habits {
        if h.p >= TRUE_HABIT_P {
            truth.habits.push((h.context.clone(), h.action.clone(), h.p));
        } else {
            truth.non_habit_contexts.push(h.context.clone());
        }
        for d in 0..cfg.days {
            for _ in 0..rng.poisson(cfg.occasions_per_day) {
                let taken = rng.f64() < h.p;
                let act = if taken { h.action.clone() } else { other(&mut rng, &h.action, &cfg.actions) };
                let (t, tags) = if cfg.free_text { (free_action(&mut rng, &h.context, &act, if taken { None } else { Some(&h.action) }), vec![]) } else { action_event(&h.context, &act) };
                push(&mut out, &mut per_day, cfg.start_day + d, "synth", "action", t, tags);
            }
        }
    }
    for c in &cfg.null_contexts {
        truth.non_habit_contexts.push(c.clone());
        for d in 0..cfg.days {
            for _ in 0..rng.poisson(cfg.occasions_per_day) {
                let act = cfg.actions[rng.below(cfg.actions.len())].clone();
                let (t, tags) = if cfg.free_text { (free_action(&mut rng, c, &act, None), vec![]) } else { action_event(c, &act) };
                push(&mut out, &mut per_day, cfg.start_day + d, "synth", "action", t, tags);
            }
        }
    }
    for (c, a, n) in &cfg.rare_contexts {
        truth.rare_contexts.push(c.clone());
        for _ in 0..*n {
            let d = rng.below(cfg.days as usize) as i64;
            let (t, tags) = if cfg.free_text { (free_action(&mut rng, c, a, None), vec![]) } else { action_event(c, a) };
            push(&mut out, &mut per_day, cfg.start_day + d, "synth", "action", t, tags);
        }
    }
    // topics
    for t in &cfg.topics {
        let mut cur = t.base_rate;
        if t.shifts.is_empty() {
            truth.steady_topics.push(t.topic.clone());
        }
        for (sd, to) in &t.shifts {
            truth.changes.push(TruthChange { topic: t.topic.clone(), day: cfg.start_day + sd, from: cur, to: *to });
            cur = *to;
        }
        for d in 0..cfg.days {
            let rate = t.shifts.iter().rev().find(|(sd, _)| *sd <= d).map(|x| x.1).unwrap_or(t.base_rate);
            for _ in 0..rng.poisson(rate) {
                if cfg.free_text {
                    let text = free_topic(&mut rng, &t.topic, false);
                    push(&mut out, &mut per_day, cfg.start_day + d, "synth", "note", text, vec![]);
                } else {
                    push(&mut out, &mut per_day, cfg.start_day + d, "synth", "note", format!("Thought about {} today", t.topic), vec![format!("topic:{}", t.topic)]);
                }
            }
        }
    }
    // stances
    for s in &cfg.stances {
        let (text, topic) = stance_text(s);
        let id = push(&mut out, &mut per_day, cfg.start_day + s.day, "synth", "statement", text, vec![]);
        truth.stances.push(TruthStance { event_id: id, day: cfg.start_day + s.day, topic });
    }
    // noise
    let signal = out.len();
    let n_noise = (cfg.noise * signal as f64).round() as usize;
    let ctxs: Vec<String> = cfg.habits.iter().map(|h| h.context.clone()).chain(cfg.null_contexts.iter().cloned()).collect();
    for _ in 0..n_noise {
        let d = rng.below(cfg.days as usize) as i64;
        let r = rng.f64();
        if r < 0.4 {
            let c = &ctxs[rng.below(ctxs.len())];
            let a = cfg.actions[rng.below(cfg.actions.len())].clone();
            let (t, tags) = if cfg.free_text { (free_action(&mut rng, c, &a, None), vec![]) } else { action_event(c, &a) };
            push(&mut out, &mut per_day, cfg.start_day + d, "synth-noise", "action", t, tags);
        } else if r < 0.8 {
            let t = &cfg.topics[rng.below(cfg.topics.len())].topic;
            if cfg.free_text {
                let text = free_topic(&mut rng, t, true);
                push(&mut out, &mut per_day, cfg.start_day + d, "synth-noise", "note", text, vec![]);
            } else {
                push(&mut out, &mut per_day, cfg.start_day + d, "synth-noise", "note", format!("Noise mention of {t}"), vec![format!("topic:{t}")]);
            }
        } else {
            let t = if cfg.free_text && rng.f64() < 0.6 { DISTRACTORS[rng.below(DISTRACTORS.len())].to_string() } else { NEAR_MISS[rng.below(NEAR_MISS.len())].to_string() };
            push(&mut out, &mut per_day, cfg.start_day + d, "synth-noise", "note", t, vec![]);
        }
    }
    (out, truth)
}

/// Event views (as the learners see them) without going through a log.
pub fn to_evs(events: &[NewEvent]) -> Vec<Ev> {
    let mut v: Vec<Ev> = events
        .iter()
        .map(|e| {
            let ts = normalize_ts(&e.occurred_at).expect("valid");
            Ev {
                id: event_id(&e.source, &ts, &e.text),
                day: dates::day_of(&ts).expect("date"),
                ts,
                kind: e.kind.clone(),
                source: e.source.clone(),
                text: e.text.clone(),
                tags: e.tags.clone(),
            }
        })
        .collect();
    v.sort_by(|a, b| (&a.ts, &a.id).cmp(&(&b.ts, &b.id)));
    v
}
