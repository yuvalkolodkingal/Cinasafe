//! Language and flag tags in subtitle file names (`Movie.he.srt`,
//! `Movie.heb.forced.srt`, `Subs/2_English.srt`, `סרט.עברית.srt`).

/// ISO 639-1 code for a file-name token, if it names a language.
pub fn language_from_token(token: &str) -> Option<&'static str> {
    let t = token.trim().to_lowercase();
    let code = match t.as_str() {
        "he" | "heb" | "iw" | "hebrew" | "עברית" | "עב" => "he",
        "ar" | "ara" | "arabic" | "عربي" | "العربية" | "عربية" => "ar",
        "en" | "eng" | "english" | "אנגלית" => "en",
        "fa" | "per" | "fas" | "persian" | "farsi" | "فارسی" => "fa",
        "ru" | "rus" | "russian" | "русский" | "רוסית" => "ru",
        "fr" | "fre" | "fra" | "french" | "français" | "francais" | "צרפתית" => "fr",
        "es" | "spa" | "spanish" | "español" | "espanol" | "ספרדית" => "es",
        "de" | "ger" | "deu" | "german" | "deutsch" | "גרמנית" => "de",
        "it" | "ita" | "italian" | "italiano" => "it",
        "pt" | "por" | "portuguese" | "português" | "pt-br" | "ptbr" | "brazilian" => "pt",
        "nl" | "dut" | "nld" | "dutch" => "nl",
        "tr" | "tur" | "turkish" | "türkçe" => "tr",
        "el" | "gre" | "ell" | "greek" => "el",
        "pl" | "pol" | "polish" => "pl",
        "ro" | "rum" | "ron" | "romanian" => "ro",
        "hu" | "hun" | "hungarian" => "hu",
        "cs" | "cze" | "ces" | "czech" => "cs",
        "sv" | "swe" | "swedish" => "sv",
        "da" | "dan" | "danish" => "da",
        "no" | "nor" | "nob" | "norwegian" => "no",
        "fi" | "fin" | "finnish" => "fi",
        "uk" | "ukr" | "ukrainian" => "uk",
        "ja" | "jpn" | "japanese" => "ja",
        "ko" | "kor" | "korean" => "ko",
        "zh" | "chi" | "zho" | "chinese" | "chs" | "cht" => "zh",
        "th" | "tha" | "thai" => "th",
        // Not "hi": in subtitle names that means hearing-impaired.
        "hin" | "hindi" => "hi",
        "yi" | "yid" | "yiddish" | "יידיש" => "yi",
        "am" | "amh" | "amharic" => "am",
        _ => return None,
    };
    Some(code)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubFlags {
    pub forced: bool,
    /// Subtitles for the deaf and hard of hearing.
    pub sdh: bool,
}

fn flag_from_token(token: &str, flags: &mut SubFlags) -> bool {
    match token.to_lowercase().as_str() {
        "forced" | "foreign" => flags.forced = true,
        "sdh" | "hi" | "cc" | "hearing" => flags.sdh = true,
        _ => return false,
    }
    true
}

/// Language and flags from the end of a subtitle stem. Looks at no more than
/// the last four tokens, so title words aren't mistaken for tags.
pub fn tags_from_stem(stem: &str) -> (Option<&'static str>, SubFlags) {
    let mut flags = SubFlags::default();
    let mut lang = None;
    let tokens: Vec<&str> = stem.split(['.', '_', ' ', '-', '[', ']', '(', ')']).filter(|t| !t.is_empty()).collect();
    for t in tokens.iter().rev().take(4) {
        if flag_from_token(t, &mut flags) {
            continue;
        }
        if lang.is_none() {
            if let Some(l) = language_from_token(t) {
                lang = Some(l);
                continue;
            }
        }
        // `2_English`: a numeric track prefix is fine; anything else ends the tag run.
        if !t.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
    }
    (lang, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(tags_from_stem("Inception.2010.1080p.he").0, Some("he"));
        assert_eq!(
            tags_from_stem("Inception.2010.1080p.heb.forced"),
            (Some("he"), SubFlags { forced: true, sdh: false })
        );
        assert_eq!(tags_from_stem("2_English"), (Some("en"), SubFlags::default()));
        assert_eq!(tags_from_stem("Movie.en.sdh"), (Some("en"), SubFlags { forced: false, sdh: true }));
        assert_eq!(tags_from_stem("Movie.HI.eng"), (Some("en"), SubFlags { forced: false, sdh: true }));
        assert_eq!(tags_from_stem("סרט.עברית").0, Some("he"));
        assert_eq!(tags_from_stem("Inception.2010.1080p").0, None);
        // "It" the movie: the title token is not inspected past a non-tag token.
        assert_eq!(tags_from_stem("It.2017.1080p").0, None);
    }
}
