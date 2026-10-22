//! External subtitle handling on import (brief section 7): charset detection
//! and conversion to UTF-8, language detection from the text itself, and the
//! optional "Fix RTL punctuation" rewrite.

use encoding_rs::Encoding;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubFormat {
    Srt,
    Ass,
    Ssa,
    Vtt,
}

impl SubFormat {
    pub fn from_extension(ext: &str) -> Option<SubFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "srt" => Some(SubFormat::Srt),
            "ass" => Some(SubFormat::Ass),
            "ssa" => Some(SubFormat::Ssa),
            "vtt" => Some(SubFormat::Vtt),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            SubFormat::Srt => "srt",
            SubFormat::Ass => "ass",
            SubFormat::Ssa => "ssa",
            SubFormat::Vtt => "vtt",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DecodedText {
    /// UTF-8 text without a byte-order mark.
    pub text: String,
    /// WHATWG name of the encoding the file was in, e.g. `windows-1255`.
    pub source_encoding: &'static str,
    /// Some bytes were invalid in the detected encoding and were replaced.
    pub had_errors: bool,
}

/// Detects the charset (BOM, then UTF-8, then `chardetng`) and converts to UTF-8.
/// Typical non-UTF-8 inputs are Windows-1255, Windows-1256 and ISO-8859-8.
pub fn decode(bytes: &[u8]) -> DecodedText {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (text, had_errors) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return DecodedText { text: text.into_owned(), source_encoding: enc.name(), had_errors };
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return DecodedText { text: s.to_owned(), source_encoding: "UTF-8", had_errors: false };
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let enc = detector.guess(None, chardetng::Utf8Detection::Allow);
    let (text, had_errors) = enc.decode_without_bom_handling(bytes);
    DecodedText { text: text.into_owned(), source_encoding: enc.name(), had_errors }
}

/// Lines of displayed text, without numbering, timings, headers or styling.
pub fn cue_text_lines(text: &str, format: SubFormat) -> Vec<String> {
    let mut out = Vec::new();
    match format {
        SubFormat::Srt | SubFormat::Vtt => {
            let mut in_header = format == SubFormat::Vtt;
            for line in text.lines() {
                let l = line.trim();
                if in_header {
                    // WebVTT: everything up to the first cue timing is header/notes.
                    if l.contains("-->") {
                        in_header = false;
                    }
                    continue;
                }
                if l.is_empty() || l.contains("-->") || l.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                out.push(strip_tags(l));
            }
        }
        SubFormat::Ass | SubFormat::Ssa => {
            for line in text.lines() {
                let Some(rest) = line.strip_prefix("Dialogue:") else { continue };
                // Text is the 10th field; it may itself contain commas.
                if let Some(t) = rest.splitn(10, ',').nth(9) {
                    for part in t.split("\\N").flat_map(|p| p.split("\\n")) {
                        out.push(strip_tags(part.trim()));
                    }
                }
            }
        }
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth_angle = false;
    let mut depth_brace = false;
    for c in s.chars() {
        match c {
            '<' if !depth_brace => depth_angle = true,
            '>' if depth_angle => depth_angle = false,
            '{' if !depth_angle => depth_brace = true,
            '}' if depth_brace => depth_brace = false,
            _ if !depth_angle && !depth_brace => out.push(c),
            _ => {}
        }
    }
    out
}

/// Language guessed from the script the text is written in. Only scripts that
/// identify a language well enough are reported (Hebrew, Arabic, Persian,
/// Greek, Thai); Latin and Cyrillic text returns `None`.
pub fn detect_language(text: &str, format: SubFormat) -> Option<&'static str> {
    let (mut letters, mut hebrew, mut arabic, mut persian, mut greek, mut thai) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    for line in cue_text_lines(text, format) {
        for c in line.chars().filter(|c| c.is_alphabetic()) {
            letters += 1;
            match c {
                '\u{0590}'..='\u{05FF}' | '\u{FB1D}'..='\u{FB4F}' => hebrew += 1,
                // Letters used in Persian but not Arabic: پ چ ژ گ and Persian kaf/yeh.
                'پ' | 'چ' | 'ژ' | 'گ' | 'ک' | 'ی' => {
                    arabic += 1;
                    persian += 1
                }
                '\u{0600}'..='\u{06FF}'
                | '\u{0750}'..='\u{077F}'
                | '\u{FB50}'..='\u{FDFF}'
                | '\u{FE70}'..='\u{FEFF}' => arabic += 1,
                '\u{0370}'..='\u{03FF}' => greek += 1,
                '\u{0E00}'..='\u{0E7F}' => thai += 1,
                _ => {}
            }
        }
    }
    if letters < 20 {
        return None;
    }
    let share = |n: u32| n * 10 >= letters * 3; // at least 30% of letters
    if share(hebrew) {
        Some("he")
    } else if share(arabic) {
        // Persian-only letters are common in Persian text and rare in Arabic.
        if persian * 20 >= arabic {
            Some("fa")
        } else {
            Some("ar")
        }
    } else if share(greek) {
        Some("el")
    } else if share(thai) {
        Some("th")
    } else {
        None
    }
}

pub fn is_rtl_language(lang: &str) -> bool {
    matches!(lang, "he" | "ar" | "fa" | "ur" | "yi")
}

const RTL_LEADING_PUNCT: &[char] = &['.', ',', '?', '!', '-'];
const RLE: char = '\u{202B}';
const PDF: char = '\u{202C}';

/// Whether to offer "Fix RTL punctuation" (brief 7.2): an RTL-language SRT or
/// WebVTT file in which cue lines start with `.`, `,`, `?`, `!` or `-`, the
/// signature of files pre-flipped for left-to-right players.
pub fn suggests_rtl_fix(text: &str, format: SubFormat, language: Option<&str>) -> bool {
    if !matches!(format, SubFormat::Srt | SubFormat::Vtt) || !language.is_some_and(is_rtl_language) {
        return false;
    }
    let lines = cue_text_lines(text, format);
    let flagged = lines.iter().filter(|l| l.trim_start().starts_with(RTL_LEADING_PUNCT)).count();
    flagged >= 3 && flagged * 20 >= lines.len()
}

/// Moves leading punctuation of every cue line to its end and wraps the line
/// in RLE…PDF (brief 7.2). SRT and WebVTT only; other formats are returned
/// unchanged. Already-fixed lines (starting with RLE) are left alone, so the
/// rewrite is idempotent.
pub fn fix_rtl_punctuation(text: &str, format: SubFormat) -> String {
    if !matches!(format, SubFormat::Srt | SubFormat::Vtt) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut in_header = format == SubFormat::Vtt;
    for line in text.split_inclusive('\n') {
        let (body, eol) = match line.strip_suffix("\r\n") {
            Some(b) => (b, "\r\n"),
            None => match line.strip_suffix('\n') {
                Some(b) => (b, "\n"),
                None => (line, ""),
            },
        };
        let t = body.trim();
        let is_cue_text = !(t.is_empty() || t.contains("-->") || t.chars().all(|c| c.is_ascii_digit()));
        if in_header {
            in_header = !t.contains("-->");
        }
        if in_header || !is_cue_text {
            out.push_str(line);
            continue;
        }
        out.push_str(&fix_line(body));
        out.push_str(eol);
    }
    out
}

fn fix_line(line: &str) -> String {
    // Keep formatting tags such as <i> or {\an8} at the very start and end.
    let (prefix, rest) = split_leading_tags(line);
    let (core, suffix) = split_trailing_tags(rest);
    if core.starts_with(RLE) {
        return line.to_owned();
    }
    let lead_len = core.len() - core.trim_start_matches(|c: char| RTL_LEADING_PUNCT.contains(&c) || c == ' ').len();
    let (lead, body) = core.split_at(lead_len);
    let moved: String = lead.chars().filter(|c| *c != ' ').collect();
    format!("{prefix}{RLE}{}{moved}{PDF}{suffix}", body.trim_end())
}

fn split_leading_tags(s: &str) -> (&str, &str) {
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        let close = match b[i] {
            b'<' => b'>',
            b'{' => b'}',
            _ => break,
        };
        match s[i..].find(close as char) {
            Some(j) => i += j + 1,
            None => break,
        }
    }
    s.split_at(i)
}

fn split_trailing_tags(s: &str) -> (&str, &str) {
    let mut end = s.len();
    loop {
        let t = &s[..end];
        let open = if t.ends_with('>') {
            t.rfind('<')
        } else if t.ends_with('}') {
            t.rfind('{')
        } else {
            None
        };
        match open {
            Some(j) => end = j,
            None => break,
        }
    }
    s.split_at(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HE_SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\n?מה שלומך\n\n2\n00:00:03,000 --> 00:00:04,000\n<i>.שלום לכולם</i>\n\n3\n00:00:05,000 --> 00:00:06,000\n!זה עובד\n\n4\n00:00:07,000 --> 00:00:08,000\nשורה רגילה בלי סימנים\n";

    #[test]
    fn decodes_legacy_hebrew_and_arabic() {
        for (enc, sample) in [
            (encoding_rs::WINDOWS_1255, "שלום עולם, זהו מבחן של כתוביות בעברית עם הרבה מילים כדי שהזיהוי יעבוד."),
            (encoding_rs::WINDOWS_1256, "مرحبا بالعالم، هذا اختبار للترجمة العربية مع كلمات كثيرة حتى يعمل الاكتشاف."),
        ] {
            let (bytes, _, _) = enc.encode(sample);
            let d = decode(&bytes);
            assert_eq!(d.text, sample);
            assert_eq!(d.source_encoding, enc.name());
        }
    }

    #[test]
    fn decodes_boms_and_utf8() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("שלום".as_bytes());
        assert_eq!(decode(&b).text, "שלום");
        let mut u16le = vec![0xFF, 0xFE];
        for u in "hi".encode_utf16() {
            u16le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&u16le).text, "hi");
        assert_eq!(decode("plain".as_bytes()).source_encoding, "UTF-8");
    }

    #[test]
    fn language_from_script() {
        assert_eq!(detect_language(HE_SRT, SubFormat::Srt), Some("he"));
        let ar = "1\n00:00:01,000 --> 00:00:02,000\nمرحبا بالعالم، هذا اختبار طويل\n";
        assert_eq!(detect_language(ar, SubFormat::Srt), Some("ar"));
        let fa =
            "1\n00:00:01,000 --> 00:00:02,000\nسلام دنیا، این یک آزمایش برای زیرنویس فارسی است که چگونه کار می کند\n";
        assert_eq!(detect_language(fa, SubFormat::Srt), Some("fa"));
        let en = "1\n00:00:01,000 --> 00:00:02,000\nHello world, this is an English line\n";
        assert_eq!(detect_language(en, SubFormat::Srt), None);
        let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\an8}שלום, עולם\\Nזו שורה שנייה בעברית\n";
        assert_eq!(detect_language(ass, SubFormat::Ass), Some("he"));
    }

    #[test]
    fn rtl_fix_offer_and_rewrite() {
        assert!(suggests_rtl_fix(HE_SRT, SubFormat::Srt, Some("he")));
        assert!(!suggests_rtl_fix(HE_SRT, SubFormat::Srt, Some("en")));
        assert!(!suggests_rtl_fix(HE_SRT, SubFormat::Ass, Some("he")));

        let fixed = fix_rtl_punctuation(HE_SRT, SubFormat::Srt);
        assert!(fixed.contains("\u{202B}מה שלומך?\u{202C}\n"));
        assert!(fixed.contains("<i>\u{202B}שלום לכולם.\u{202C}</i>\n"));
        assert!(fixed.contains("\u{202B}שורה רגילה בלי סימנים\u{202C}\n"));
        // Numbering and timings untouched; idempotent.
        assert!(fixed.starts_with("1\n00:00:01,000 --> 00:00:02,000\n"));
        assert_eq!(fix_rtl_punctuation(&fixed, SubFormat::Srt), fixed);
    }

    #[test]
    fn rtl_fix_keeps_crlf_and_vtt_header() {
        let vtt = "WEBVTT\r\nKind: captions\r\n\r\n00:01.000 --> 00:02.000\r\n?מה\r\n";
        let fixed = fix_rtl_punctuation(vtt, SubFormat::Vtt);
        assert_eq!(fixed, "WEBVTT\r\nKind: captions\r\n\r\n00:01.000 --> 00:02.000\r\n\u{202B}מה?\u{202C}\r\n");
    }
}
