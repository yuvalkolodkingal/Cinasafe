//! Ranking search hits against what the file name said.

use super::Candidate;
use crate::import::normalize_title;

/// Below this the match is shown to the user as a suggestion, not applied.
pub const AUTO_ACCEPT: f32 = 0.75;
/// The best hit must beat the runner-up by this much to be applied automatically.
pub const AUTO_MARGIN: f32 = 0.08;

#[derive(Clone, Debug)]
pub struct Ranked {
    pub candidate: Candidate,
    /// 0..=1.
    pub confidence: f32,
}

fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (normalize_title(a), normalize_title(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    // Leading "the"/"a" and punctuation differences are common; Jaro-Winkler
    // forgives prefixes, Levenshtein penalises extra words. Take the gentler.
    let lev = strsim::normalized_levenshtein(&a, &b) as f32;
    let jw = strsim::jaro_winkler(&a, &b) as f32 * 0.95;
    lev.max(jw)
}

/// Scores and sorts `candidates` (best first).
pub fn rank(title: &str, year: Option<u16>, candidates: Vec<Candidate>) -> Vec<Ranked> {
    let max_pop = candidates.iter().map(|c| c.popularity).fold(0.0f64, f64::max);
    let mut ranked: Vec<Ranked> = candidates
        .into_iter()
        .map(|c| {
            let sim = similarity(title, &c.title)
                .max(c.original_title.as_deref().map(|o| similarity(title, o)).unwrap_or(0.0));
            let year_term = match (year, c.year) {
                (Some(q), Some(y)) if q == y => 0.15,
                (Some(q), Some(y)) if q.abs_diff(y) == 1 => 0.05,
                (Some(_), Some(_)) => -0.25,
                (Some(_), None) => -0.05,
                _ => 0.0,
            };
            // Popularity only breaks ties between otherwise equal hits.
            let pop_term = if max_pop > 0.0 { (c.popularity / max_pop) as f32 * 0.1 } else { 0.0 };
            let confidence = (sim * 0.85 + year_term + pop_term).clamp(0.0, 1.0);
            Ranked { candidate: c, confidence }
        })
        .collect();
    ranked.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    ranked
}

/// Whether the top hit is safe to apply without asking.
pub fn is_confident(ranked: &[Ranked]) -> bool {
    match ranked {
        [best, rest @ ..] => {
            best.confidence >= AUTO_ACCEPT
                && rest.first().is_none_or(|second| best.confidence - second.confidence >= AUTO_MARGIN)
        }
        [] => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, title: &str, year: Option<u16>, pop: f64) -> Candidate {
        Candidate {
            provider: "test",
            id: id.into(),
            title: title.into(),
            original_title: None,
            year,
            overview: None,
            popularity: pop,
            poster: None,
        }
    }

    #[test]
    fn prefers_title_and_year_then_popularity() {
        let r = rank(
            "Inception",
            Some(2010),
            vec![
                cand("short", "Inception: The Cobol Job", Some(2010), 3.0),
                cand("main", "Inception", Some(2010), 95.0),
                cand("other", "Inception", Some(2014), 0.6),
            ],
        );
        assert_eq!(r[0].candidate.id, "main");
        assert!(is_confident(&r), "{r:?}");
    }

    #[test]
    fn remakes_need_the_year() {
        let hits = vec![cand("1990", "Total Recall", Some(1990), 30.0), cand("2012", "Total Recall", Some(2012), 25.0)];
        let r = rank("Total Recall", Some(2012), hits.clone());
        assert_eq!(r[0].candidate.id, "2012");
        assert!(is_confident(&r));
        // Without a year the two are too close to call.
        assert!(!is_confident(&rank("Total Recall", None, hits)));
    }

    #[test]
    fn matches_original_title() {
        let mut c = cand("f", "Fauda", Some(2015), 10.0);
        c.original_title = Some("פאודה".into());
        let r = rank("פאודה", None, vec![c]);
        assert!(r[0].confidence > 0.8);
    }

    #[test]
    fn weak_match_is_not_confident() {
        let r = rank("Some Home Video", None, vec![cand("x", "Home Alone", Some(1990), 50.0)]);
        assert!(!is_confident(&r));
    }
}
