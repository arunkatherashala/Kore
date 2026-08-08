//! ISO 639-1 language catalog, script detection, and multilingual queries for KORE-self.

use crate::world_types::WorldAnswer;

#[derive(Debug, Clone, Copy)]
pub struct LanguageMeta {
    pub code: &'static str,
    pub name: &'static str,
    pub autonym: &'static str,
}

/// All ISO 639-1 two-letter codes. ~7,000+ languages exist worldwide (Ethnologue).
pub const ISO639_1: &[LanguageMeta] = &[
    LanguageMeta { code: "aa", name: "Afar", autonym: "Qafar af" },
    LanguageMeta { code: "ab", name: "Abkhaz", autonym: "Abkhaz" },
    LanguageMeta { code: "ae", name: "Avestan", autonym: "Avestan" },
    LanguageMeta { code: "af", name: "Afrikaans", autonym: "Afrikaans" },
    LanguageMeta { code: "ak", name: "Akan", autonym: "Akan" },
    LanguageMeta { code: "am", name: "Amharic", autonym: "Amharic" },
    LanguageMeta { code: "an", name: "Aragonese", autonym: "Aragonese" },
    LanguageMeta { code: "ar", name: "Arabic", autonym: "Arabic" },
    LanguageMeta { code: "as", name: "Assamese", autonym: "Assamese" },
    LanguageMeta { code: "av", name: "Avar", autonym: "Avar" },
    LanguageMeta { code: "ay", name: "Aymara", autonym: "Aymara" },
    LanguageMeta { code: "az", name: "Azerbaijani", autonym: "Azerbaijani" },
    LanguageMeta { code: "ba", name: "Bashkir", autonym: "Bashkir" },
    LanguageMeta { code: "be", name: "Belarusian", autonym: "Belarusian" },
    LanguageMeta { code: "bg", name: "Bulgarian", autonym: "Bulgarian" },
    LanguageMeta { code: "bh", name: "Bihari", autonym: "Bihari" },
    LanguageMeta { code: "bi", name: "Bislama", autonym: "Bislama" },
    LanguageMeta { code: "bm", name: "Bambara", autonym: "Bambara" },
    LanguageMeta { code: "bn", name: "Bengali", autonym: "Bengali" },
    LanguageMeta { code: "bo", name: "Tibetan", autonym: "Tibetan" },
    LanguageMeta { code: "br", name: "Breton", autonym: "Breton" },
    LanguageMeta { code: "bs", name: "Bosnian", autonym: "Bosnian" },
    LanguageMeta { code: "ca", name: "Catalan", autonym: "Catalan" },
    LanguageMeta { code: "ce", name: "Chechen", autonym: "Chechen" },
    LanguageMeta { code: "ch", name: "Chamorro", autonym: "Chamorro" },
    LanguageMeta { code: "co", name: "Corsican", autonym: "Corsican" },
    LanguageMeta { code: "cr", name: "Cree", autonym: "Cree" },
    LanguageMeta { code: "cs", name: "Czech", autonym: "Czech" },
    LanguageMeta { code: "cu", name: "Old Church Slavonic", autonym: "Slavonic" },
    LanguageMeta { code: "cv", name: "Chuvash", autonym: "Chuvash" },
    LanguageMeta { code: "cy", name: "Welsh", autonym: "Cymraeg" },
    LanguageMeta { code: "da", name: "Danish", autonym: "Dansk" },
    LanguageMeta { code: "de", name: "German", autonym: "Deutsch" },
    LanguageMeta { code: "dv", name: "Divehi", autonym: "Divehi" },
    LanguageMeta { code: "dz", name: "Dzongkha", autonym: "Dzongkha" },
    LanguageMeta { code: "ee", name: "Ewe", autonym: "Ewe" },
    LanguageMeta { code: "el", name: "Greek", autonym: "Greek" },
    LanguageMeta { code: "en", name: "English", autonym: "English" },
    LanguageMeta { code: "eo", name: "Esperanto", autonym: "Esperanto" },
    LanguageMeta { code: "es", name: "Spanish", autonym: "Espanol" },
    LanguageMeta { code: "et", name: "Estonian", autonym: "Estonian" },
    LanguageMeta { code: "eu", name: "Basque", autonym: "Basque" },
    LanguageMeta { code: "fa", name: "Persian", autonym: "Persian" },
    LanguageMeta { code: "ff", name: "Fula", autonym: "Fula" },
    LanguageMeta { code: "fi", name: "Finnish", autonym: "Finnish" },
    LanguageMeta { code: "fj", name: "Fijian", autonym: "Fijian" },
    LanguageMeta { code: "fo", name: "Faroese", autonym: "Faroese" },
    LanguageMeta { code: "fr", name: "French", autonym: "Francais" },
    LanguageMeta { code: "fy", name: "Western Frisian", autonym: "Frisian" },
    LanguageMeta { code: "ga", name: "Irish", autonym: "Gaeilge" },
    LanguageMeta { code: "gd", name: "Scottish Gaelic", autonym: "Gaelic" },
    LanguageMeta { code: "gl", name: "Galician", autonym: "Galician" },
    LanguageMeta { code: "gn", name: "Guarani", autonym: "Guarani" },
    LanguageMeta { code: "gu", name: "Gujarati", autonym: "Gujarati" },
    LanguageMeta { code: "gv", name: "Manx", autonym: "Manx" },
    LanguageMeta { code: "ha", name: "Hausa", autonym: "Hausa" },
    LanguageMeta { code: "he", name: "Hebrew", autonym: "Hebrew" },
    LanguageMeta { code: "hi", name: "Hindi", autonym: "Hindi" },
    LanguageMeta { code: "ho", name: "Hiri Motu", autonym: "Hiri Motu" },
    LanguageMeta { code: "hr", name: "Croatian", autonym: "Croatian" },
    LanguageMeta { code: "ht", name: "Haitian Creole", autonym: "Creole" },
    LanguageMeta { code: "hu", name: "Hungarian", autonym: "Hungarian" },
    LanguageMeta { code: "hy", name: "Armenian", autonym: "Armenian" },
    LanguageMeta { code: "hz", name: "Herero", autonym: "Herero" },
    LanguageMeta { code: "ia", name: "Interlingua", autonym: "Interlingua" },
    LanguageMeta { code: "id", name: "Indonesian", autonym: "Indonesian" },
    LanguageMeta { code: "ie", name: "Interlingue", autonym: "Interlingue" },
    LanguageMeta { code: "ig", name: "Igbo", autonym: "Igbo" },
    LanguageMeta { code: "ii", name: "Nuosu", autonym: "Nuosu" },
    LanguageMeta { code: "ik", name: "Inupiaq", autonym: "Inupiaq" },
    LanguageMeta { code: "io", name: "Ido", autonym: "Ido" },
    LanguageMeta { code: "is", name: "Icelandic", autonym: "Icelandic" },
    LanguageMeta { code: "it", name: "Italian", autonym: "Italiano" },
    LanguageMeta { code: "iu", name: "Inuktitut", autonym: "Inuktitut" },
    LanguageMeta { code: "ja", name: "Japanese", autonym: "Japanese" },
    LanguageMeta { code: "jv", name: "Javanese", autonym: "Javanese" },
    LanguageMeta { code: "ka", name: "Georgian", autonym: "Georgian" },
    LanguageMeta { code: "kg", name: "Kongo", autonym: "Kongo" },
    LanguageMeta { code: "ki", name: "Kikuyu", autonym: "Kikuyu" },
    LanguageMeta { code: "kj", name: "Kuanyama", autonym: "Kuanyama" },
    LanguageMeta { code: "kk", name: "Kazakh", autonym: "Kazakh" },
    LanguageMeta { code: "kl", name: "Kalaallisut", autonym: "Kalaallisut" },
    LanguageMeta { code: "km", name: "Khmer", autonym: "Khmer" },
    LanguageMeta { code: "kn", name: "Kannada", autonym: "Kannada" },
    LanguageMeta { code: "ko", name: "Korean", autonym: "Korean" },
    LanguageMeta { code: "kr", name: "Kanuri", autonym: "Kanuri" },
    LanguageMeta { code: "ks", name: "Kashmiri", autonym: "Kashmiri" },
    LanguageMeta { code: "ku", name: "Kurdish", autonym: "Kurdish" },
    LanguageMeta { code: "kv", name: "Komi", autonym: "Komi" },
    LanguageMeta { code: "kw", name: "Cornish", autonym: "Cornish" },
    LanguageMeta { code: "ky", name: "Kyrgyz", autonym: "Kyrgyz" },
    LanguageMeta { code: "la", name: "Latin", autonym: "Latina" },
    LanguageMeta { code: "lb", name: "Luxembourgish", autonym: "Luxembourgish" },
    LanguageMeta { code: "lg", name: "Ganda", autonym: "Ganda" },
    LanguageMeta { code: "li", name: "Limburgish", autonym: "Limburgish" },
    LanguageMeta { code: "ln", name: "Lingala", autonym: "Lingala" },
    LanguageMeta { code: "lo", name: "Lao", autonym: "Lao" },
    LanguageMeta { code: "lt", name: "Lithuanian", autonym: "Lithuanian" },
    LanguageMeta { code: "lu", name: "Luba-Katanga", autonym: "Luba" },
    LanguageMeta { code: "lv", name: "Latvian", autonym: "Latvian" },
    LanguageMeta { code: "mg", name: "Malagasy", autonym: "Malagasy" },
    LanguageMeta { code: "mh", name: "Marshallese", autonym: "Marshallese" },
    LanguageMeta { code: "mi", name: "Maori", autonym: "Maori" },
    LanguageMeta { code: "mk", name: "Macedonian", autonym: "Macedonian" },
    LanguageMeta { code: "ml", name: "Malayalam", autonym: "Malayalam" },
    LanguageMeta { code: "mn", name: "Mongolian", autonym: "Mongolian" },
    LanguageMeta { code: "mr", name: "Marathi", autonym: "Marathi" },
    LanguageMeta { code: "ms", name: "Malay", autonym: "Malay" },
    LanguageMeta { code: "mt", name: "Maltese", autonym: "Maltese" },
    LanguageMeta { code: "my", name: "Burmese", autonym: "Burmese" },
    LanguageMeta { code: "na", name: "Nauru", autonym: "Nauru" },
    LanguageMeta { code: "nb", name: "Norwegian Bokmal", autonym: "Bokmal" },
    LanguageMeta { code: "nd", name: "North Ndebele", autonym: "Ndebele" },
    LanguageMeta { code: "ne", name: "Nepali", autonym: "Nepali" },
    LanguageMeta { code: "ng", name: "Ndonga", autonym: "Ndonga" },
    LanguageMeta { code: "nl", name: "Dutch", autonym: "Dutch" },
    LanguageMeta { code: "nn", name: "Norwegian Nynorsk", autonym: "Nynorsk" },
    LanguageMeta { code: "no", name: "Norwegian", autonym: "Norwegian" },
    LanguageMeta { code: "nr", name: "South Ndebele", autonym: "Ndebele" },
    LanguageMeta { code: "nv", name: "Navajo", autonym: "Navajo" },
    LanguageMeta { code: "ny", name: "Chichewa", autonym: "Chichewa" },
    LanguageMeta { code: "oc", name: "Occitan", autonym: "Occitan" },
    LanguageMeta { code: "oj", name: "Ojibwe", autonym: "Ojibwe" },
    LanguageMeta { code: "om", name: "Oromo", autonym: "Oromo" },
    LanguageMeta { code: "or", name: "Odia", autonym: "Odia" },
    LanguageMeta { code: "os", name: "Ossetian", autonym: "Ossetian" },
    LanguageMeta { code: "pa", name: "Punjabi", autonym: "Punjabi" },
    LanguageMeta { code: "pi", name: "Pali", autonym: "Pali" },
    LanguageMeta { code: "pl", name: "Polish", autonym: "Polish" },
    LanguageMeta { code: "ps", name: "Pashto", autonym: "Pashto" },
    LanguageMeta { code: "pt", name: "Portuguese", autonym: "Portuguese" },
    LanguageMeta { code: "qu", name: "Quechua", autonym: "Quechua" },
    LanguageMeta { code: "rm", name: "Romansh", autonym: "Romansh" },
    LanguageMeta { code: "rn", name: "Kirundi", autonym: "Kirundi" },
    LanguageMeta { code: "ro", name: "Romanian", autonym: "Romanian" },
    LanguageMeta { code: "ru", name: "Russian", autonym: "Russian" },
    LanguageMeta { code: "rw", name: "Kinyarwanda", autonym: "Kinyarwanda" },
    LanguageMeta { code: "sa", name: "Sanskrit", autonym: "Sanskrit" },
    LanguageMeta { code: "sc", name: "Sardinian", autonym: "Sardinian" },
    LanguageMeta { code: "sd", name: "Sindhi", autonym: "Sindhi" },
    LanguageMeta { code: "se", name: "Northern Sami", autonym: "Sami" },
    LanguageMeta { code: "sg", name: "Sango", autonym: "Sango" },
    LanguageMeta { code: "si", name: "Sinhala", autonym: "Sinhala" },
    LanguageMeta { code: "sk", name: "Slovak", autonym: "Slovak" },
    LanguageMeta { code: "sl", name: "Slovenian", autonym: "Slovenian" },
    LanguageMeta { code: "sm", name: "Samoan", autonym: "Samoan" },
    LanguageMeta { code: "sn", name: "Shona", autonym: "Shona" },
    LanguageMeta { code: "so", name: "Somali", autonym: "Somali" },
    LanguageMeta { code: "sq", name: "Albanian", autonym: "Albanian" },
    LanguageMeta { code: "sr", name: "Serbian", autonym: "Serbian" },
    LanguageMeta { code: "ss", name: "Swati", autonym: "Swati" },
    LanguageMeta { code: "st", name: "Southern Sotho", autonym: "Sotho" },
    LanguageMeta { code: "su", name: "Sundanese", autonym: "Sundanese" },
    LanguageMeta { code: "sv", name: "Swedish", autonym: "Swedish" },
    LanguageMeta { code: "sw", name: "Swahili", autonym: "Swahili" },
    LanguageMeta { code: "ta", name: "Tamil", autonym: "Tamil" },
    LanguageMeta { code: "te", name: "Telugu", autonym: "Telugu" },
    LanguageMeta { code: "tg", name: "Tajik", autonym: "Tajik" },
    LanguageMeta { code: "th", name: "Thai", autonym: "Thai" },
    LanguageMeta { code: "ti", name: "Tigrinya", autonym: "Tigrinya" },
    LanguageMeta { code: "tk", name: "Turkmen", autonym: "Turkmen" },
    LanguageMeta { code: "tl", name: "Tagalog", autonym: "Tagalog" },
    LanguageMeta { code: "tn", name: "Tswana", autonym: "Tswana" },
    LanguageMeta { code: "to", name: "Tongan", autonym: "Tongan" },
    LanguageMeta { code: "tr", name: "Turkish", autonym: "Turkish" },
    LanguageMeta { code: "ts", name: "Tsonga", autonym: "Tsonga" },
    LanguageMeta { code: "tt", name: "Tatar", autonym: "Tatar" },
    LanguageMeta { code: "tw", name: "Twi", autonym: "Twi" },
    LanguageMeta { code: "ty", name: "Tahitian", autonym: "Tahitian" },
    LanguageMeta { code: "ug", name: "Uyghur", autonym: "Uyghur" },
    LanguageMeta { code: "uk", name: "Ukrainian", autonym: "Ukrainian" },
    LanguageMeta { code: "ur", name: "Urdu", autonym: "Urdu" },
    LanguageMeta { code: "uz", name: "Uzbek", autonym: "Uzbek" },
    LanguageMeta { code: "ve", name: "Venda", autonym: "Venda" },
    LanguageMeta { code: "vi", name: "Vietnamese", autonym: "Vietnamese" },
    LanguageMeta { code: "vo", name: "Volapuk", autonym: "Volapuk" },
    LanguageMeta { code: "wa", name: "Walloon", autonym: "Walloon" },
    LanguageMeta { code: "wo", name: "Wolof", autonym: "Wolof" },
    LanguageMeta { code: "xh", name: "Xhosa", autonym: "Xhosa" },
    LanguageMeta { code: "yi", name: "Yiddish", autonym: "Yiddish" },
    LanguageMeta { code: "yo", name: "Yoruba", autonym: "Yoruba" },
    LanguageMeta { code: "za", name: "Zhuang", autonym: "Zhuang" },
    LanguageMeta { code: "zh", name: "Chinese", autonym: "Chinese" },
    LanguageMeta { code: "zu", name: "Zulu", autonym: "Zulu" },
];

pub fn wikipedia_rotation() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("English", "en", "Science"),
        ("Spanish", "es", "computación"),
        ("French", "fr", "philosophie"),
        ("German", "de", "Mathematik"),
        ("Japanese", "ja", "宇宙"),
        ("Chinese", "zh", "数学"),
        ("Portuguese", "pt", "ciência"),
        ("Russian", "ru", "физика"),
        ("Arabic", "ar", "رياضيات"),
        ("Hindi", "hi", "विज्ञान"),
        ("Korean", "ko", "우주"),
        ("Italian", "it", "filosofia"),
        ("Dutch", "nl", "wetenschap"),
        ("Polish", "pl", "fizyka"),
        ("Turkish", "tr", "matematik"),
        ("Swedish", "sv", "vetenskap"),
        ("Latin", "la", "mathematica"),
        ("Greek", "el", "φιλοσοφία"),
        ("Sanskrit", "sa", "गणित"),
        ("Persian", "fa", "ریاضیات"),
        ("Hebrew", "he", "מתמטיקה"),
        ("Swahili", "sw", "hisabati"),
        ("Bengali", "bn", "গণিত"),
        ("Vietnamese", "vi", "toán_học"),
        ("Indonesian", "id", "matematika"),
        ("Thai", "th", "คณิตศาสตร์"),
        ("Telugu", "te", "గణితం"),
        ("Tamil", "ta", "கணிதம்"),
        ("Urdu", "ur", "ریاضی"),
        ("Ukrainian", "uk", "математика"),
        ("Czech", "cs", "fyzika"),
        ("Romanian", "ro", "știință"),
        ("Hungarian", "hu", "tudomány"),
        ("Finnish", "fi", "tiede"),
        ("Norwegian", "no", "vitenskap"),
        ("Danish", "da", "videnskab"),
        ("Malay", "ms", "sains"),
        ("Filipino", "tl", "agham"),
        ("Esperanto", "eo", "scienco"),
        ("Catalan", "ca", "ciència"),
        ("Basque", "eu", "zientzia"),
        ("Galician", "gl", "ciencia"),
        ("Serbian", "sr", "наука"),
        ("Bulgarian", "bg", "наука"),
        ("Slovak", "sk", "veda"),
        ("Croatian", "hr", "znanost"),
        ("Lithuanian", "lt", "mokslas"),
        ("Latvian", "lv", "zinātne"),
        ("Estonian", "et", "teadus"),
        ("Icelandic", "is", "vísindi"),
        ("Irish", "ga", "eolaíocht"),
        ("Welsh", "cy", "gwyddoniaeth"),
        ("Afrikaans", "af", "wetenskap"),
        ("Amharic", "am", "ሳይንስ"),
        ("Georgian", "ka", "მეცნიერება"),
        ("Armenian", "hy", "գիտություն"),
        ("Kazakh", "kk", "ғылым"),
        ("Uzbek", "uz", "fan"),
        ("Mongolian", "mn", "шинжлэх_ухaan"),
        ("Nepali", "ne", "विज्ञान"),
        ("Sinhala", "si", "විද්‍යාව"),
        ("Khmer", "km", "វិទ្យាសាស្ត្រ"),
        ("Lao", "lo", "ວິທະຍາສາດ"),
        ("Burmese", "my", "သိပ္ပံ"),
        ("Javanese", "jv", "élmu"),
        ("Yoruba", "yo", "sáyẹ́nsì"),
        ("Hausa", "ha", "kimiyya"),
        ("Zulu", "zu", "isayensi"),
        ("Xhosa", "xh", "isayensi"),
        ("Maori", "mi", "pūtaiao"),
    ]
}

pub fn catalog_summary() -> String {
    let mut lines = vec![
        format!(
            "KORE language catalog: {} ISO 639-1 codes indexed.",
            ISO639_1.len()
        ),
        "Living languages worldwide: ~7,000 (Ethnologue); ISO 639-3 lists 7,000+.".into(),
        format!(
            "KORE Wikipedia rotation: {} language editions.",
            wikipedia_rotation().len()
        ),
        String::new(),
        "Sample (code | English | autonym):".into(),
    ];
    for lang in ISO639_1.iter().take(25) {
        lines.push(format!("  {} | {} | {}", lang.code, lang.name, lang.autonym));
    }
    lines.push(format!(
        "  … and {} more — use self_world_catalog action=languages.",
        ISO639_1.len().saturating_sub(25)
    ));
    lines.join("\n")
}

pub fn full_language_list() -> String {
    let mut out = String::from("ISO 639-1 languages in KORE-self:\n");
    for lang in ISO639_1 {
        out.push_str(&format!("{} | {} | {}\n", lang.code, lang.name, lang.autonym));
    }
    out
}

pub fn lookup_by_name_or_code(token: &str) -> Option<&'static LanguageMeta> {
    let t = token.to_lowercase();
    ISO639_1.iter().find(|l| {
        l.code == t
            || l.name.to_lowercase() == t
            || l.name.to_lowercase().contains(&t)
            || (t.len() >= 3 && l.name.to_lowercase().starts_with(&t))
    })
}

pub fn detect_script(text: &str) -> &'static str {
    let mut latin = 0u32;
    let mut cjk = 0u32;
    let mut arabic = 0u32;
    let mut cyrillic = 0u32;
    let mut devanagari = 0u32;
    let mut other = 0u32;
    for ch in text.chars() {
        let u = ch as u32;
        if ch.is_ascii_alphabetic() {
            latin += 1;
        } else if (0x4E00..=0x9FFF).contains(&u)
            || (0x3040..=0x30FF).contains(&u)
            || (0xAC00..=0xD7AF).contains(&u)
        {
            cjk += 1;
        } else if (0x0600..=0x06FF).contains(&u) {
            arabic += 1;
        } else if (0x0400..=0x04FF).contains(&u) {
            cyrillic += 1;
        } else if (0x0900..=0x097F).contains(&u) {
            devanagari += 1;
        } else if ch.is_alphabetic() {
            other += 1;
        }
    }
    let max = latin.max(cjk).max(arabic).max(cyrillic).max(devanagari).max(other);
    if max == 0 {
        return "Latin (default) / undetermined";
    }
    if max == latin {
        "Latin script"
    } else if max == cjk {
        "CJK (Chinese / Japanese / Korean)"
    } else if max == arabic {
        "Arabic script"
    } else if max == cyrillic {
        "Cyrillic script"
    } else if max == devanagari {
        "Devanagari script"
    } else {
        "Mixed or other Unicode scripts"
    }
}

fn hello_in(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some("Hello"),
        "es" => Some("Hola"),
        "fr" => Some("Bonjour"),
        "de" => Some("Hallo"),
        "it" => Some("Ciao"),
        "pt" => Some("Ola"),
        "ru" => Some("Zdravstvuyte"),
        "ja" => Some("Konnichiwa"),
        "zh" => Some("Ni hao"),
        "ko" => Some("Annyeonghaseyo"),
        "ar" => Some("Marhaba"),
        "hi" => Some("Namaste"),
        "te" => Some("Namaskaram"),
        "bn" => Some("Nomoshkar"),
        "tr" => Some("Merhaba"),
        "vi" => Some("Xin chao"),
        "th" => Some("Sawadee"),
        "sw" => Some("Habari"),
        "he" => Some("Shalom"),
        "el" => Some("Geia sou"),
        "la" => Some("Salve"),
        "sa" => Some("Namaste"),
        "nl" => Some("Hallo"),
        "pl" => Some("Czesc"),
        "sv" => Some("Hej"),
        "fi" => Some("Hei"),
        "id" => Some("Halo"),
        "ms" => Some("Helo"),
        "tl" => Some("Kumusta"),
        "uk" => Some("Pryvit"),
        "cs" => Some("Ahoj"),
        "ro" => Some("Buna"),
        "hu" => Some("Szia"),
        "da" => Some("Hej"),
        "no" => Some("Hei"),
        "fa" => Some("Salam"),
        "ur" => Some("Salam"),
        "ta" => Some("Vanakkam"),
        "mr" => Some("Namaskar"),
        "gu" => Some("Namaste"),
        "pa" => Some("Sat sri akal"),
        "yo" => Some("Bawo"),
        "zu" => Some("Sawubona"),
        "mi" => Some("Kia ora"),
        _ => None,
    }
}

pub fn try_language_query(
    problem: &str,
    lower: &str,
    steps: &mut Vec<String>,
) -> Option<WorldAnswer> {
    let lang_topic = lower.contains("language")
        || lower.contains("languages")
        || lower.contains("translate")
        || lower.contains("script")
        || lower.contains("iso 639")
        || lower.contains("multilingual")
        || lower.contains("greeting");

    if !lang_topic {
        return None;
    }

    if lower.contains("how many") && lower.contains("language") {
        steps.push("Ethnologue + ISO registry facts.".into());
        return Some(WorldAnswer::new(
            "languages_count",
            format!(
                "Living human languages: ~7,000 (Ethnologue — real count, not all stored in KORE).\n\
                 KORE learns via Wikipedia: use KORE_CONTINUOUS=1 + KORE_LANG_FAST=1 + KORE_LANG_BURST=8 for fastest ingest.\n\
                 ISO 639-1 codes in KORE: {}.\n\
                 Wikipedia rotation: {} editions.",
                ISO639_1.len(),
                wikipedia_rotation().len()
            ),
            0.93,
        ));
    }

    if lower.contains("list") && (lower.contains("language") || lower.contains("iso")) {
        steps.push("Emit full ISO 639-1 catalog.".into());
        return Some(WorldAnswer::new("languages_list", full_language_list(), 0.95));
    }

    if lower.contains("hello") || lower.contains("greeting") {
        for word in problem.split(|c: char| !c.is_alphanumeric()) {
            if word.len() < 2 {
                continue;
            }
            if let Some(meta) = lookup_by_name_or_code(word) {
                if let Some(h) = hello_in(meta.code) {
                    steps.push(format!("Greeting for {} ({})", meta.name, meta.code));
                    return Some(WorldAnswer::new(
                        "language_greeting",
                        format!("'Hello' in {} ({}) -> {}", meta.name, meta.code, h),
                        0.9,
                    ));
                }
            }
        }
    }

    if lower.contains("detect") || lower.contains("what language") || lower.contains("which script") {
        let sample = problem.split(':').nth(1).unwrap_or(problem).trim();
        let script = detect_script(sample);
        steps.push("Unicode script heuristic.".into());
        return Some(WorldAnswer::new(
            "language_detect",
            format!(
                "Script: {}\nSample: {} chars",
                script,
                sample.chars().count()
            ),
            0.75,
        ));
    }

    if lower.contains("catalog") || lower.contains("639") {
        return Some(WorldAnswer::new(
            "languages_catalog",
            catalog_summary(),
            0.94,
        ));
    }

    None
}

/// Fast multilingual ingest: on when `KORE_LANG_FAST=1` or continuous mode (override with `KORE_LANG_FAST=0`).
pub fn lang_ingest_policy(continuous: bool) -> (bool, usize) {
    let fast = match std::env::var("KORE_LANG_FAST") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        _ => continuous,
    };
    let burst = std::env::var("KORE_LANG_BURST")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(if fast { 6 } else { 1 })
        .clamp(1, 12);
    (fast, burst)
}

/// Fetch Wikipedia REST summary (title, extract). Uses reqwest + allowlist.
pub fn fetch_wikipedia_summary(
    lang_code: &str,
    topic: &str,
    timeout_secs: u64,
) -> Option<(String, String)> {
    crate::net_fetch::fetch_wikipedia_summary(lang_code, topic, timeout_secs)
}
