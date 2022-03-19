use rust_stemmers::{Algorithm, Stemmer};

include!(concat!(env!("OUT_DIR"), "/synonyms.rs"));

pub fn normalize_and_extract_words(s: &str) -> Vec<String> {
    let stemmer = Stemmer::create(Algorithm::French);
    let ascii = deunicode::deunicode(s);
    let ascii_without_symbols: String = ascii.chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
        .collect();
    let filtered_words = ascii_without_symbols
        .split(|c: char| c.is_ascii_whitespace())
        .map(|w| {
            let w = w.to_lowercase();
            // The stemmer converts "cs" (Cauchy-Schwarz) into "c" which we do not want.
            if w == "cs" {
                w
            } else {
                stemmer.stem(&w).to_string()
            }
        })
        .filter(|w| !w.is_empty())
        // Ignore common words
        .filter(|w| {
            ![
                "le", "la", "de", "un", "et", "en", "que", "dan", "pour", "ce", "qui",
                "ne", "se", "sur", "pas", "par", "on", "mais", "ou", "comm", "il", "est",
                "du", "lorsqu", "une",
            ]
            .contains(&w.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ");

    // Canonicalization
    let mut canonicalized = filtered_words;
    for (canonical, synonyms) in SYNONYMS.iter() {
        for s in synonyms.iter() {
            let mut search = 0;
            while let Some(mut p) = canonicalized.get(search..).and_then(|c| c.find(s)) {
                p += search;

                search = p + s.len();
                // Check if we are at a word boundary
                if (p != 0 && canonicalized.as_bytes()[p - 1] != b' ')
                    || (p + s.len() < canonicalized.len()
                        && canonicalized.as_bytes()[p + s.len()] != b' ')
                {
                    continue;
                }

                canonicalized = canonicalized.get(..p).unwrap().to_owned()
                    + canonical
                    + canonicalized.get((p + s.len())..).unwrap();
            }
        }
    }

    let words = canonicalized
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_owned())
        .collect();
    words
}

#[cfg(test)]
mod tests {
    use super::normalize_and_extract_words;

    #[test]
    fn acronyms() {
        assert_eq!(normalize_and_extract_words("t.e.s.t."), vec!["test"]);
    }
}
