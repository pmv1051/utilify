//! Fuzzy matching helpers: the same song often exists under several URIs
//! (re-releases, deluxe editions, remasters), so tools also compare by a
//! normalized title + primary artist.

/// Lower-case a title, drop edition noise such as "(feat. X)", "[Remastered]",
/// "- 2011 Remaster", "- Live", strip punctuation and collapse whitespace.
pub fn normalize_title(title: &str) -> String {
    let mut s = title.to_lowercase();

    // Anything after " - " is almost always a version qualifier.
    if let Some(idx) = s.find(" - ") {
        s.truncate(idx);
    }
    // Remove bracketed qualifiers.
    s = strip_brackets(&s, '(', ')');
    s = strip_brackets(&s, '[', ']');
    // "feat." without brackets.
    for marker in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(idx) = s.find(marker) {
            s.truncate(idx);
        }
    }

    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_brackets(s: &str, open: char, close: char) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth = depth.saturating_sub(1);
        } else if depth == 0 {
            out.push(c);
        }
    }
    out
}

/// Primary artist, lower-cased. `artists` is the comma-joined display string.
pub fn primary_artist_key(artists: &str) -> String {
    artists
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// Key that groups the same song across different URIs.
pub fn name_key(title: &str, artists: &str) -> String {
    format!("{}|{}", normalize_title(title), primary_artist_key(artists))
}

/// Levenshtein-based similarity in 0..=1 (1 = identical) on normalized text.
pub fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let dist = levenshtein(&a, &b);
    1.0 - dist as f64 / a.len().max(b.len()) as f64
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_edition_noise() {
        assert_eq!(normalize_title("Blinding Lights - Remastered 2021"), "blinding lights");
        assert_eq!(normalize_title("Sunflower (feat. Swae Lee)"), "sunflower");
        assert_eq!(normalize_title("Sunflower feat. Swae Lee"), "sunflower");
        assert_eq!(normalize_title("Hey Jude [Live]"), "hey jude");
        assert_eq!(normalize_title("  Don't   Stop Me Now!! "), "don t stop me now");
    }

    #[test]
    fn name_key_groups_versions() {
        assert_eq!(
            name_key("Bohemian Rhapsody - 2011 Remaster", "Queen"),
            name_key("Bohemian Rhapsody", "Queen, Freddie Mercury")
        );
        assert_ne!(name_key("Hello", "Adele"), name_key("Hello", "Lionel Richie"));
    }

    #[test]
    fn similarity_scores() {
        assert_eq!(similarity("abc", "abc"), 1.0);
        assert!(similarity("kitten", "sitting") > 0.5);
        assert!(similarity("kitten", "sitting") < 0.7);
        assert_eq!(similarity("", ""), 1.0);
        assert_eq!(similarity("abc", ""), 0.0);
    }
}
