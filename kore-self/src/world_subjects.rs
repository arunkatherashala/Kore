//! Subject taxonomy and factual solvers across humanities, life science, geography, and more.

use crate::world_types::WorldAnswer;

struct SubjectArea {
    name: &'static str,
    keywords: &'static [&'static str],
}

const SUBJECTS: &[SubjectArea] = &[
    SubjectArea {
        name: "Mathematics",
        keywords: &["math", "algebra", "geometry", "calculus", "statistics", "probability", "theorem"],
    },
    SubjectArea {
        name: "Physics",
        keywords: &["physics", "force", "energy", "quantum", "relativity", "thermodynamics"],
    },
    SubjectArea {
        name: "Chemistry",
        keywords: &["chemistry", "molecule", "atom", "reaction", "periodic", "acid", "base"],
    },
    SubjectArea {
        name: "Biology",
        keywords: &["biology", "cell", "dna", "gene", "evolution", "organism", "photosynthesis", "ecosystem"],
    },
    SubjectArea {
        name: "Earth & Environmental Science",
        keywords: &["geology", "weather", "climate", "ocean", "earthquake", "volcano", "atmosphere"],
    },
    SubjectArea {
        name: "Astronomy & Space Science",
        keywords: &["astronomy", "space", "planet", "galaxy", "cosmos", "orbit", "star"],
    },
    SubjectArea {
        name: "Computer Science",
        keywords: &["computer", "programming", "algorithm", "binary", "hexadecimal", "software", "data structure"],
    },
    SubjectArea {
        name: "Engineering",
        keywords: &["engineering", "mechanical", "electrical", "civil", "aerospace", "robotics"],
    },
    SubjectArea {
        name: "Medicine & Health",
        keywords: &["medicine", "anatomy", "disease", "health", "physiology", "pharmacology"],
    },
    SubjectArea {
        name: "Geography",
        keywords: &["geography", "capital", "continent", "country", "river", "mountain"],
    },
    SubjectArea {
        name: "History",
        keywords: &["history", "war", "ancient", "medieval", "revolution", "empire", "century"],
    },
    SubjectArea {
        name: "Philosophy",
        keywords: &["philosophy", "ethics", "logic", "metaphysics", "epistemology", "existential"],
    },
    SubjectArea {
        name: "Psychology",
        keywords: &["psychology", "cognitive", "behavior", "memory", "emotion", "personality"],
    },
    SubjectArea {
        name: "Economics & Business",
        keywords: &["economics", "gdp", "inflation", "market", "finance", "trade", "business"],
    },
    SubjectArea {
        name: "Law & Politics",
        keywords: &["law", "politics", "government", "democracy", "constitution", "human rights"],
    },
    SubjectArea {
        name: "Sociology & Anthropology",
        keywords: &["sociology", "anthropology", "culture", "society", "ritual", "kinship"],
    },
    SubjectArea {
        name: "Linguistics",
        keywords: &["linguistics", "grammar", "phonetics", "syntax", "semantics", "language family"],
    },
    SubjectArea {
        name: "Literature & Writing",
        keywords: &["literature", "poetry", "novel", "author", "writing", "fiction"],
    },
    SubjectArea {
        name: "Arts & Music",
        keywords: &["art", "music", "painting", "sculpture", "composer", "theater", "dance"],
    },
    SubjectArea {
        name: "Religion & Mythology",
        keywords: &["religion", "mythology", "theology", "buddhism", "christianity", "islam", "hinduism"],
    },
    SubjectArea {
        name: "Education",
        keywords: &["education", "pedagogy", "learning", "curriculum", "teaching"],
    },
    SubjectArea {
        name: "Agriculture & Food Science",
        keywords: &["agriculture", "crop", "soil", "nutrition", "food science", "farming"],
    },
];

pub fn classify(lower: &str) -> Vec<&'static str> {
    SUBJECTS
        .iter()
        .filter(|s| s.keywords.iter().any(|k| lower.contains(k)))
        .map(|s| s.name)
        .collect()
}

pub fn taxonomy_summary() -> String {
    let mut out = String::from("KORE world subject taxonomy (keyword-routed):\n");
    for s in SUBJECTS {
        out.push_str(&format!("• {} — e.g. {}\n", s.name, s.keywords[0]));
    }
    out.push_str("\nUse self_solve for math/science; self_world_catalog for full lists; self_fetch wikipedia for deep articles.");
    out
}

pub fn try_subjects(problem: &str, lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if let Some(a) = try_geography(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_biology(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_earth_science(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_computer_science(problem, lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_economics(problem, lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_history(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_medicine(lower, steps) {
        return Some(a);
    }
    if let Some(a) = try_philosophy(lower, steps) {
        return Some(a);
    }

    let hits = classify(lower);
    if hits.len() == 1 && !lower.chars().any(|c| c.is_ascii_digit()) {
        steps.push("Single subject area matched.".into());
        return Some(WorldAnswer::new(
            "subject_pointer",
            format!(
                "This looks like {}.\n\
                 KORE routes: math/physics/chemistry/space via self_solve; \
                 languages via self_world_catalog; deep facts via self_fetch (wikipedia).\n\n{}",
                hits[0],
                taxonomy_summary()
            ),
            0.55,
        ));
    }

    None
}

fn try_geography(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if !lower.contains("capital") {
        return None;
    }
    const CAPITALS: &[(&str, &str)] = &[
        ("france", "Paris"),
        ("germany", "Berlin"),
        ("italy", "Rome"),
        ("spain", "Madrid"),
        ("india", "New Delhi"),
        ("japan", "Tokyo"),
        ("china", "Beijing"),
        ("brazil", "Brasilia"),
        ("mexico", "Mexico City"),
        ("canada", "Ottawa"),
        ("australia", "Canberra"),
        ("russia", "Moscow"),
        ("uk", "London"),
        ("britain", "London"),
        ("england", "London"),
        ("usa", "Washington, D.C."),
        ("united states", "Washington, D.C."),
        ("south korea", "Seoul"),
        ("north korea", "Pyongyang"),
        ("egypt", "Cairo"),
        ("nigeria", "Abuja"),
        ("south africa", "Pretoria"),
        ("argentina", "Buenos Aires"),
        ("turkey", "Ankara"),
        ("saudi arabia", "Riyadh"),
        ("israel", "Jerusalem"),
        ("pakistan", "Islamabad"),
        ("bangladesh", "Dhaka"),
        ("indonesia", "Jakarta"),
        ("thailand", "Bangkok"),
        ("vietnam", "Hanoi"),
        ("philippines", "Manila"),
        ("poland", "Warsaw"),
        ("ukraine", "Kyiv"),
        ("sweden", "Stockholm"),
        ("norway", "Oslo"),
        ("finland", "Helsinki"),
        ("greece", "Athens"),
        ("portugal", "Lisbon"),
        ("netherlands", "Amsterdam"),
        ("belgium", "Brussels"),
        ("switzerland", "Bern"),
        ("austria", "Vienna"),
        ("ireland", "Dublin"),
        ("new zealand", "Wellington"),
        ("singapore", "Singapore"),
        ("uae", "Abu Dhabi"),
        ("iran", "Tehran"),
        ("iraq", "Baghdad"),
        ("ethiopia", "Addis Ababa"),
        ("kenya", "Nairobi"),
        ("colombia", "Bogota"),
        ("chile", "Santiago"),
        ("peru", "Lima"),
        ("venezuela", "Caracas"),
    ];
    for (country, cap) in CAPITALS {
        if lower.contains(country) {
            steps.push(format!("Capital lookup: {country}"));
            return Some(WorldAnswer::new(
                "geography_capital",
                format!("Capital of {}: {}", capitalize(country), cap),
                0.92,
            ));
        }
    }
    None
}

fn try_biology(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("photosynthesis") {
        steps.push("Biology: photosynthesis summary.".into());
        return Some(WorldAnswer::new(
            "biology_photosynthesis",
            "Photosynthesis: 6CO₂ + 6H₂O + light → C₆H₁₂O₆ + 6O₂. \
             Occurs in chloroplasts; converts light energy to chemical energy (glucose).",
            0.9,
        ));
    }
    if lower.contains("dna") && (lower.contains("base") || lower.contains("pair") || lower.contains("structure")) {
        return Some(WorldAnswer::new(
            "biology_dna",
            "DNA: double helix; bases A-T, G-C (complementary pairs). \
             Stores genetic code as nucleotide sequences.",
            0.91,
        ));
    }
    if lower.contains("mitosis") || lower.contains("meiosis") {
        return Some(WorldAnswer::new(
            "biology_cell_division",
            "Mitosis: 1 cell → 2 identical diploid cells (growth/repair). \
             Meiosis: 1 cell → 4 haploid gametes (sexual reproduction).",
            0.89,
        ));
    }
    None
}

fn try_earth_science(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("layers of the earth") || lower.contains("earth's layers") {
        steps.push("Earth structure.".into());
        return Some(WorldAnswer::new(
            "earth_science_layers",
            "Earth layers (inside → out): inner core, outer core, mantle, crust.",
            0.93,
        ));
    }
    if lower.contains("water cycle") {
        return Some(WorldAnswer::new(
            "earth_science_water_cycle",
            "Water cycle: evaporation → condensation → precipitation → collection/runoff → repeat.",
            0.9,
        ));
    }
    None
}

fn try_computer_science(
    problem: &str,
    lower: &str,
    steps: &mut Vec<String>,
) -> Option<WorldAnswer> {
    if lower.contains("binary") && (lower.contains("decimal") || lower.contains("to dec")) {
        let digits: String = problem
            .chars()
            .filter(|c| *c == '0' || *c == '1')
            .collect();
        if !digits.is_empty() {
            if let Ok(v) = u64::from_str_radix(&digits, 2) {
                steps.push(format!("Binary {digits} → decimal"));
                return Some(WorldAnswer::new(
                    "cs_binary",
                    format!("{digits} (binary) = {v} (decimal)"),
                    0.94,
                ));
            }
        }
    }
    if lower.contains("hex") && lower.contains("decimal") {
        let hex: String = problem
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .collect();
        if hex.len() >= 2 {
            if let Ok(v) = u64::from_str_radix(&hex, 16) {
                return Some(WorldAnswer::new(
                    "cs_hex",
                    format!("0x{hex} = {v} decimal"),
                    0.93,
                ));
            }
        }
    }
    None
}

fn try_economics(problem: &str, lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("compound interest") || (lower.contains("interest") && lower.contains("principal")) {
        let n: Vec<f64> = parse_numbers(problem);
        if n.len() >= 3 {
            let (p, r, t) = (n[0], n[1], n[2]);
            let amount = p * (1.0 + r / 100.0).powf(t);
            steps.push("A = P(1 + r/100)^t".into());
            return Some(WorldAnswer::new(
                "economics_compound_interest",
                format!("Amount after {t} periods ≈ {amount:.2} (P={p}, r={r}%)"),
                0.86,
            ));
        }
    }
    None
}

fn try_history(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("world war ii") || lower.contains("world war 2") || lower.contains("ww2") {
        steps.push("History: WWII dates.".into());
        return Some(WorldAnswer::new(
            "history_wwii",
            "World War II: 1939–1945 (global conflict; Axis vs Allies).",
            0.9,
        ));
    }
    if lower.contains("world war i") || lower.contains("ww1") {
        return Some(WorldAnswer::new(
            "history_wwi",
            "World War I: 1914–1918.",
            0.9,
        ));
    }
    if lower.contains("independence") && lower.contains("india") {
        return Some(WorldAnswer::new(
            "history_india_independence",
            "India independence: 15 August 1947.",
            0.88,
        ));
    }
    None
}

fn try_medicine(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("bpm") || (lower.contains("heart rate") && lower.contains("normal")) {
        steps.push("Normal resting heart rate range.".into());
        return Some(WorldAnswer::new(
            "medicine_heart_rate",
            "Normal resting heart rate (adults): about 60–100 beats per minute.",
            0.85,
        ));
    }
    None
}

fn try_philosophy(lower: &str, steps: &mut Vec<String>) -> Option<WorldAnswer> {
    if lower.contains("socratic method") {
        steps.push("Philosophy: Socratic method.".into());
        return Some(WorldAnswer::new(
            "philosophy_socratic",
            "Socratic method: disciplined questioning to examine beliefs and expose contradictions.",
            0.88,
        ));
    }
    None
}

fn parse_numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            cur.push(ch);
        } else if !cur.is_empty() {
            if let Ok(v) = cur.parse() {
                out.push(v);
            }
            cur.clear();
        }
    }
    if !cur.is_empty() {
        if let Ok(v) = cur.parse() {
            out.push(v);
        }
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
