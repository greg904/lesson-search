use rust_stemmers::{Algorithm, Stemmer};

include!(concat!(env!("OUT_DIR"), "/synonyms.rs"));

pub fn normalize_and_extract_words(s: &str) -> Vec<String> {
    // Generic normalization.
    let stemmer = Stemmer::create(Algorithm::French);
    let mut normalized = s
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| {
            let w = w.to_lowercase();
            // The stemmer converts "cs" (Cauchy-Schwarz) into "c" which we do not want.
            if w == "cs" {
                w
            } else {
                stemmer.stem(&w).to_lowercase()
            }
        })
        .map(|w| {
            deunicode::deunicode(&w)
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|p| p.len() > 1)
                // Ignore common words
                .filter(|p| {
                    ![
                        "le", "la", "de", "un", "et", "en", "que", "dan", "pour", "ce", "qui",
                        "ne", "se", "sur", "pas", "par", "on", "mais", "ou", "comm", "il", "est",
                        "du", "lorsqu", "une",
                    ]
                    .contains(p)
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    // Canonicalize special words.
    for (canonical, synonyms) in SYNONYMS.iter() {
        for s in synonyms.iter() {
            let mut search = 0;
            while let Some(mut p) = normalized.get(search..).and_then(|c| c.find(s)) {
                p += search;

                search = p + s.len();
                // Check if we are at a word boundary
                if (p != 0 && normalized.as_bytes()[p - 1] != b' ')
                    || (p + s.len() < normalized.len()
                        && normalized.as_bytes()[p + s.len()] != b' ')
                {
                    continue;
                }

                normalized = normalized.get(..p).unwrap().to_owned()
                    + canonical
                    + normalized.get((p + s.len())..).unwrap();
            }
        }
    }

    // Split words.
    normalized
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_owned())
        .collect()
}
