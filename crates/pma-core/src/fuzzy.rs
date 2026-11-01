//! Fuzzy matching for the front ends' filters and the project chooser: a
//! needle matches when its characters appear in order, and a match scores
//! higher when they sit together or start words. `app.js` holds the same
//! rules, so both front ends keep and order rows alike.

/// How well `needle` matches `hay`; `None` when its characters do not appear
/// in order. Case is ignored. An empty needle matches everything at 0.
pub fn score(needle: &str, hay: &str) -> Option<u32> {
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = hay.chars().flat_map(char::to_lowercase).collect();
    // From each place the needle could start, the rest greedily; the best
    // start wins, so `au` prefers the word `audio` to the middle of `cyfaust`.
    // Where the match starts costs nothing: the rows are short.
    let starts = hay.iter().enumerate().filter(|(_, h)| **h == needle[0]);
    starts
        .filter_map(|(start, _)| {
            let mut score: i64 = 0;
            let mut last = start;
            for (n, &c) in needle.iter().enumerate() {
                let found = match n {
                    0 => start,
                    _ => hay[last + 1..].iter().position(|&h| h == c)? + last + 1,
                };
                score += 2;
                if n > 0 {
                    match found == last + 1 {
                        true => score += 3,
                        false => score -= (found - last - 1).min(10) as i64,
                    }
                }
                if found == 0 || is_break(hay[found - 1]) {
                    score += 2;
                }
                last = found;
            }
            Some(score.max(0) as u32)
        })
        .max()
}

fn is_break(c: char) -> bool {
    !c.is_alphanumeric()
}

/// The indices of `rows` that `needle` matches, best first; ties keep their
/// order. Every index, in order, for an empty needle. A row matches when one
/// of its cells does: across a whole row, five letters find a path through
/// most rows, and `audio` kept 11 of 113 projects rather than the 5 tagged.
pub fn rank<T>(needle: &str, rows: &[T], cells: impl Fn(&T) -> Vec<String>) -> Vec<usize> {
    let mut scored: Vec<(u32, usize)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            cells(r)
                .iter()
                .filter_map(|c| score(needle, c))
                .max()
                .map(|s| (s, i))
        })
        .collect();
    if !needle.is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_must_appear_in_order() {
        assert!(score("mgo", "margo").is_some());
        assert!(score("ogm", "margo").is_none());
        assert_eq!(score("", "anything"), Some(0));
        assert!(score("Margo", "MARGO audio").is_some(), "case is ignored");
    }

    #[test]
    fn together_and_at_word_starts_scores_higher() {
        let run = score("mar", "margo").unwrap();
        let spread = score("mar", "m-a-r").unwrap();
        assert!(run > spread, "{run} > {spread}");
        let start = score("au", "cyfaust audio").unwrap();
        let inside = score("au", "cyfaust").unwrap();
        assert!(start > inside, "{start} > {inside}");
    }

    #[test]
    fn rank_puts_the_best_first_and_keeps_ties_in_order() {
        let rows = ["cyllama audio", "margo", "cyfaust audio", "nanosrv"];
        let cell = |r: &&str| vec![r.to_string()];
        assert_eq!(rank("aud", &rows, cell), [0, 2]);
        assert_eq!(rank("", &rows, cell), [0, 1, 2, 3]);
        assert_eq!(rank("margo", &rows, cell), [1]);
    }

    /// A match is within one cell, not across the row.
    #[test]
    fn a_row_matches_by_one_of_its_cells() {
        let rows = [
            vec!["argdec", "T5", "clear"],
            vec!["cyfaust", "audio", "clear"],
        ];
        let cells = |r: &Vec<&str>| r.iter().map(|c| c.to_string()).collect();
        assert_eq!(rank("audio", &rows, cells), [1]);
        // `argd` is in one cell of the first row and no cell of the second,
        // though its letters appear in order across the second's cells.
        assert_eq!(rank("argd", &rows, cells), [0]);
    }
}
