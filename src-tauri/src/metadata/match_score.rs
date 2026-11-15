const MAX_DURATION_DRIFT_MS: f64 = 5_000.0;
const MAX_UNCREDITED_DRIFT_MS: f64 = 2_000.0;

pub struct Candidate {
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: f64,
}

pub struct Matched {
    pub score: f64,
}

pub fn best_candidate(
    target_title: &str,
    target_artist: &str,
    target_album: Option<&str>,
    target_duration_ms: f64,
    cands: &[Candidate],
) -> Option<(usize, Matched)> {
    let (target_title, guests) = parse_title(target_title);
    if target_title.is_empty() {
        return None;
    }
    let target_credits = Credits::parse([target_artist], guests);
    let target_album = album_label(target_album);

    let mut best: Option<(usize, Matched)> = None;
    for (index, candidate) in cands.iter().enumerate() {
        let duration_delta = duration_delta(target_duration_ms, candidate.duration_ms);
        if duration_delta.is_some_and(|delta| delta > MAX_DURATION_DRIFT_MS) {
            continue;
        }

        let (title, guests) = parse_title(&candidate.title);
        if title != target_title {
            continue;
        }
        let credits = Credits::parse(candidate.artists.iter().map(String::as_str), guests);
        let album = album_label(candidate.album.as_deref());
        let album_penalty = match (&target_album, &album) {
            (Some(target), Some(candidate)) if target == candidate => 0.0,
            (Some(_), Some(_)) => 10.0,
            _ => 6.0,
        };

        // A missing artist needs corroboration from the same album and both durations.
        let artist_penalty = if target_credits.primary.is_empty() || credits.primary.is_empty() {
            if album_penalty != 0.0
                || !duration_delta.is_some_and(|delta| delta <= MAX_UNCREDITED_DRIFT_MS)
                || (!target_credits.featured.is_empty()
                    && !credits.featured.is_empty()
                    && target_credits.featured != credits.featured)
            {
                continue;
            }
            20.0
        } else if let Some(penalty) = target_credits.penalty_against(&credits) {
            penalty
        } else {
            continue;
        };

        let duration_penalty = duration_delta.map_or(5.0, |delta| delta / 1_000.0);
        let score = 100.0 - artist_penalty - album_penalty - duration_penalty;

        // Keep the provider's order when two candidates have equal evidence.
        if best
            .as_ref()
            .is_none_or(|(_, previous)| score > previous.score)
        {
            best = Some((index, Matched { score }));
        }
    }
    best
}

fn duration_delta(target: f64, candidate: f64) -> Option<f64> {
    (target.is_finite() && candidate.is_finite() && target > 0.0 && candidate > 0.0)
        .then(|| (target - candidate).abs())
}

fn album_label(value: Option<&str>) -> Option<String> {
    value
        .map(|label| normalized_words(&fold_text(label)))
        .filter(|label| !label.is_empty())
}

// Keep all recording annotations in the title and move explicit guests to the credits.
fn parse_title(value: &str) -> (String, Vec<String>) {
    let value = fold_text(value);
    let mut base = value.trim();
    let mut suffixes = Vec::new();
    let mut featured = Vec::new();
    while let Some((head, suffix)) = title_suffix(base) {
        if let Some(artists) = feature_prefix(suffix) {
            add_artist_names(artists, &mut featured);
        } else {
            let suffix = without_feature(suffix, &mut featured);
            let suffix = normalized_words(suffix);
            if !suffix.is_empty() {
                suffixes.push(suffix);
            }
        }
        base = head.trim_end();
    }

    let mut name = normalized_words(base);
    if !name.is_empty() {
        for suffix in suffixes.iter().rev() {
            name.push(' ');
            name.push_str(suffix);
        }
    }
    (name, featured)
}

fn title_suffix(value: &str) -> Option<(&str, &str)> {
    if let Some(suffix) = bracket_suffix(value) {
        return Some(suffix);
    }
    let dash_suffix = [value.rsplit_once(" - "), value.rsplit_once(['–', '—'])]
        .into_iter()
        .flatten()
        .filter(|(head, suffix)| !head.trim().is_empty() && !suffix.trim().is_empty())
        .max_by_key(|(head, _)| head.len());
    dash_suffix.or_else(|| feature_suffix(value))
}

fn bracket_suffix(value: &str) -> Option<(&str, &str)> {
    let closing = value.chars().next_back()?;
    let opening = match closing {
        ')' => '(',
        ']' => '[',
        '】' => '【',
        _ => return None,
    };
    let end = value.len() - closing.len_utf8();
    let mut level = 1;
    for (index, character) in value[..end].char_indices().rev() {
        if character == closing {
            level += 1;
        } else if character == opening {
            level -= 1;
            if level == 0 {
                let head = value[..index].trim_end();
                return (!head.is_empty())
                    .then_some((head, &value[index + opening.len_utf8()..end]));
            }
        }
    }
    None
}

fn feature_prefix(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let end = value.find(|c: char| !c.is_alphabetic())?;
    if !matches!(&value[..end], "feat" | "ft" | "featuring") {
        return None;
    }
    let artists =
        value[end..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ':'));
    (!artists.is_empty()).then_some(artists)
}

fn feature_suffix(value: &str) -> Option<(&str, &str)> {
    for (index, _) in value.match_indices(char::is_whitespace) {
        let (head, suffix) = value.split_at(index);
        if !head.trim().is_empty() && feature_prefix(suffix).is_some() {
            return Some((head, suffix));
        }
    }
    None
}

fn without_feature<'a>(value: &'a str, featured: &mut Vec<String>) -> &'a str {
    if let Some((head, artists)) = bracket_suffix(value.trim_end())
        .or_else(|| feature_suffix(value))
        .and_then(|(head, suffix)| feature_prefix(suffix).map(|artists| (head, artists)))
    {
        add_artist_names(artists, featured);
        head
    } else {
        value
    }
}

struct Credits {
    primary: Vec<String>,
    featured: Vec<String>,
}

impl Credits {
    fn parse<'a>(labels: impl IntoIterator<Item = &'a str>, mut featured: Vec<String>) -> Self {
        let mut primary = Vec::new();
        for label in labels {
            let label = fold_text(label);
            let label = without_feature(&label, &mut featured);
            add_artist_names(label, &mut primary);
        }
        sort_names(&mut primary);
        sort_names(&mut featured);
        primary.retain(|name| featured.binary_search(name).is_err());
        Self { primary, featured }
    }

    fn penalty_against(&self, other: &Self) -> Option<f64> {
        if self.primary == other.primary && self.featured == other.featured {
            return Some(0.0);
        }

        // Providers may list guests alongside the primary artists or omit them.
        // A partial match is allowed only when the missing names are explicit guests.
        let (marked, plain) = match (self.featured.is_empty(), other.featured.is_empty()) {
            (false, true) => (self, other),
            (true, false) => (other, self),
            _ => return None,
        };
        if marked.primary == plain.primary {
            return Some(6.0);
        }
        let mut all_names = marked.primary.clone();
        all_names.extend_from_slice(&marked.featured);
        all_names.sort_unstable();
        (all_names == plain.primary).then_some(0.0)
    }
}

fn add_artist_names(label: &str, names: &mut Vec<String>) {
    names.extend(
        label
            .split([',', ';', '/', '&', '+', '、', '×'])
            .map(normalized_words)
            .filter(|name| !name.is_empty()),
    );
}

fn sort_names(names: &mut Vec<String>) {
    names.sort_unstable();
    names.dedup();
}

fn fold_text(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\u{ff01}'..='\u{ff5e}' => char::from((c as u32 - 0xfee0) as u8),
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

fn normalized_words(value: &str) -> String {
    let mut words = String::new();
    let mut word_break = false;
    for c in value.chars() {
        if c.is_alphanumeric() {
            if word_break {
                words.push(' ');
            }
            words.push(c);
            word_break = false;
        } else if !matches!(c, '\'' | '’' | '‘' | 'ʼ') {
            word_break = !words.is_empty();
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        title: &str,
        artists: &[&str],
        album: Option<&str>,
        duration_ms: f64,
    ) -> Candidate {
        Candidate {
            title: title.into(),
            artists: artists.iter().map(|artist| (*artist).into()).collect(),
            album: album.map(str::to_owned),
            duration_ms,
        }
    }

    #[test]
    fn accepts_exact_metadata_with_four_seconds_of_drift() {
        let hits = [candidate(
            "Blue Moon",
            &["Mira"],
            Some("Nightfall"),
            184_000.0,
        )];
        let (index, matched) =
            best_candidate("Blue Moon", "Mira", Some("Nightfall"), 180_000.0, &hits).unwrap();
        assert_eq!(index, 0);
        assert_eq!(matched.score, 96.0);
    }

    #[test]
    fn rejects_six_seconds_of_drift() {
        let hits = [candidate(
            "Blue Moon",
            &["Mira"],
            Some("Nightfall"),
            174_000.0,
        )];
        assert!(best_candidate("Blue Moon", "Mira", Some("Nightfall"), 180_000.0, &hits).is_none());
    }

    #[test]
    fn rejects_same_title_by_another_artist() {
        let hits = [
            candidate("Blue Moon", &["Maria"], Some("Nightfall"), 180_000.0),
            candidate(
                "Blue Moon",
                &["Mira", "Maria"],
                Some("Nightfall"),
                180_000.0,
            ),
        ];
        assert!(best_candidate("Blue Moon", "Mira", Some("Nightfall"), 180_000.0, &hits).is_none());
    }

    #[test]
    fn rejects_different_recording_versions() {
        for version in [
            "Blue Moon (Instrumental)",
            "Blue Moon - Karaoke Version",
            "Blue Moon — Live",
            "Blue Moon [Acoustic]",
            "Blue Moon（翻唱版）",
            "Blue Moon【纯伴奏】",
        ] {
            let hits = [candidate(version, &["Mira"], Some("Nightfall"), 180_000.0)];
            assert!(
                best_candidate("Blue Moon", "Mira", Some("Nightfall"), 180_000.0, &hits).is_none(),
                "{version}"
            );
        }
    }

    #[test]
    fn recording_annotation_can_change_punctuation_but_keeps_its_words() {
        let hits = [
            candidate(
                "Blue Moon (Live at Home)",
                &["Mira"],
                Some("Nightfall"),
                180_000.0,
            ),
            candidate(
                "Blue Moon — Live at Rome",
                &["Mira"],
                Some("Nightfall"),
                180_000.0,
            ),
        ];
        let (index, _) = best_candidate(
            "Blue Moon (Live at Rome)",
            "Mira",
            Some("Nightfall"),
            180_000.0,
            &hits,
        )
        .unwrap();
        assert_eq!(index, 1);
        assert!(best_candidate(
            "Blue Moon (Live at Rome)",
            "Mira",
            Some("Nightfall"),
            180_000.0,
            &hits[..1],
        )
        .is_none());
    }

    #[test]
    fn another_release_is_eligible_and_the_same_album_ranks_higher() {
        let hits = [
            candidate("Blue Moon", &["Mira"], Some("Greatest Hits"), 181_000.0),
            candidate("Blue Moon", &["Mira"], Some("Nightfall"), 184_000.0),
        ];
        assert!(best_candidate(
            "Blue Moon",
            "Mira",
            Some("Nightfall"),
            180_000.0,
            &hits[..1]
        )
        .is_some());
        let (index, _) =
            best_candidate("Blue Moon", "Mira", Some("Nightfall"), 180_000.0, &hits).unwrap();
        assert_eq!(index, 1);
    }

    #[test]
    fn missing_album_and_duration_are_accepted_with_matching_title_and_artist() {
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let hits = [candidate("Blue Moon", &["Mira"], None, duration)];
            let (_, matched) =
                best_candidate("Blue Moon", "Mira", Some("Nightfall"), 180_000.0, &hits).unwrap();
            assert!(matched.score.is_finite());
            assert!((0.0..100.0).contains(&matched.score));
        }
        let hits = [candidate(
            "Blue Moon",
            &["Mira"],
            Some("Nightfall"),
            180_000.0,
        )];
        assert!(best_candidate("Blue Moon", "Mira", None, 0.0, &hits).is_some());
    }

    #[test]
    fn folds_unicode_case_width_and_common_punctuation() {
        let hits = [candidate(
            "DON'T STOP, ＢÉＢÉ!",
            &["BJÖRK"],
            Some("NIGHT－FALL"),
            180_000.0,
        )];
        let (_, matched) = best_candidate(
            "Don’t Stop BÉBÉ",
            "Björk",
            Some("Night Fall"),
            180_000.0,
            &hits,
        )
        .unwrap();
        assert_eq!(matched.score, 100.0);
    }

    #[test]
    fn matches_multiple_artists_across_provider_and_local_tags() {
        let hits = [candidate("Blue Moon", &["Alex", "Mira", "Lin"], None, 0.0)];
        assert!(best_candidate("Blue Moon", "Mira；Lin、Alex", None, 0.0, &hits).is_some());
        assert!(best_candidate("Blue Moon", "Mira；Alex", None, 0.0, &hits).is_none());
    }

    #[test]
    fn matches_feature_credits_from_titles_and_artist_tags() {
        let hits = [candidate("Blue Moon", &["Alex", "Mira"], None, 0.0)];
        assert!(best_candidate("Blue Moon (feat. Alex)", "Mira", None, 0.0, &hits).is_some());
        assert!(best_candidate("Blue Moon", "Mira featuring Alex", None, 0.0, &hits).is_some());
        let hits = [
            candidate("Blue Moon (Live) [ft. Alex]", &["Mira", "Alex"], None, 0.0),
            candidate("Blue Moon (Live)", &["Alex"], None, 0.0),
        ];
        let (index, _) =
            best_candidate("Blue Moon feat. Alex (Live)", "Mira", None, 0.0, &hits).unwrap();
        assert_eq!(index, 0);
    }

    #[test]
    fn distinguishes_missing_features_from_conflicting_credits() {
        let hits = [
            candidate("Blue Moon", &["Mira feat. Lin"], None, 0.0),
            candidate("Blue Moon", &["Alex feat. Mira"], None, 0.0),
            candidate("Blue Moon", &["Mira"], None, 0.0),
            candidate("Blue Moon", &["Mira", "Alex"], None, 0.0),
        ];
        let (index, best) =
            best_candidate("Blue Moon", "Mira feat. Alex", None, 0.0, &hits).unwrap();
        assert_eq!(index, 3);
        assert!(best_candidate("Blue Moon", "Mira feat. Alex", None, 0.0, &hits[..2]).is_none());
        let (_, without_guest) =
            best_candidate("Blue Moon", "Mira feat. Alex", None, 0.0, &hits[2..3]).unwrap();
        assert!(best.score > without_guest.score);
    }

    #[test]
    fn requires_exact_short_titles_and_recording_identifiers() {
        for (title, wrong) in [
            ("七里香", "七里乡"),
            ("七里香 (Live in Taipei)", "七里乡 (Live in Taipei)"),
            (
                "A – Ann Arbor - Live Recording",
                "B – Ann Arbor - Live Recording",
            ),
            ("Everlasting, Pt. 2", "Everlasting, Pt. 3"),
            ("Nightfall Life", "Nightfall Live"),
            ("Everlasting", "Lasting Ever"),
        ] {
            let hits = [candidate(wrong, &["Mira"], Some("Nightfall"), 180_000.0)];
            assert!(
                best_candidate(title, "Mira", Some("Nightfall"), 180_000.0, &hits).is_none(),
                "{title}: {wrong}"
            );
        }
    }

    #[test]
    fn missing_artists_need_exact_title_album_and_close_known_durations() {
        let hits = [
            candidate("Blue Moon", &[], Some("Nightfall"), 181_000.0),
            candidate("Blue Moon", &["Mira"], Some("Nightfall"), 180_000.0),
        ];
        assert!(best_candidate(
            "Blue Moon",
            "Mira",
            Some("Nightfall"),
            180_000.0,
            &hits[..1]
        )
        .is_some());
        assert!(
            best_candidate("Blue Moon", "", Some("Nightfall"), 180_000.0, &hits[1..]).is_some()
        );
        assert!(best_candidate("Blue Moon", "Mira", None, 180_000.0, &hits[..1]).is_none());
        assert!(best_candidate("Blue Moon", "Mira", Some("Nightfall"), 0.0, &hits[..1]).is_none());
        assert!(best_candidate(
            "Blue Moon",
            "Mira",
            Some("Nightfall"),
            184_000.0,
            &hits[..1]
        )
        .is_none());
        assert!(best_candidate(
            "Small Moon",
            "Mira",
            Some("Nightfall"),
            180_000.0,
            &hits[..1]
        )
        .is_none());
        let (index, _) =
            best_candidate("Blue Moon", "Mira", Some("Nightfall"), 180_000.0, &hits).unwrap();
        assert_eq!(index, 1);
    }

    #[test]
    fn empty_titles_are_ineligible() {
        let hits = [
            candidate("", &["Mira"], Some("Nightfall"), 180_000.0),
            candidate("Blue Moon", &["Mira"], Some("Nightfall"), 180_000.0),
        ];
        assert!(best_candidate(
            "Blue Moon",
            "Mira",
            Some("Nightfall"),
            180_000.0,
            &hits[..1]
        )
        .is_none());
        assert!(best_candidate("", "Mira", Some("Nightfall"), 180_000.0, &hits).is_none());
        let hits = [candidate("Live", &["Mira"], None, 0.0)];
        assert!(best_candidate("♪ (Live)", "Mira", None, 0.0, &hits).is_none());
    }

    #[test]
    fn exact_titles_win_and_equal_candidates_keep_input_order() {
        let hits = [
            candidate("Everlatsing", &["Mira"], Some("Nightfall"), 180_000.0),
            candidate("Everlasting", &["Mira"], Some("Nightfall"), 180_000.0),
            candidate("Everlasting", &["Mira"], Some("Nightfall"), 180_000.0),
        ];
        let (index, matched) =
            best_candidate("Everlasting", "Mira", Some("Nightfall"), 180_000.0, &hits).unwrap();
        assert_eq!(index, 1);
        assert_eq!(matched.score, 100.0);
    }
}
