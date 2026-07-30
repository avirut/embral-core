//! The pure text work around OCR: tidying what an engine hands back,
//! deciding whether it is worth indexing at all, and squeezing it down for
//! the summary prompt's image inventory.
//!
//! The OS call itself is thin platform code (`platform/{windows,macos}/ocr.rs`);
//! everything that has an opinion about the *text* lives here, where it can
//! be tested without an OS.

/// Tidy raw engine output into a document.
///
/// Both engines answer in lines — `OcrLine` on Windows, one observation per
/// line on macOS — with whatever spacing their layout analysis produced.
/// Collapse the runs, drop the blanks, and keep the line structure: on a
/// slide the line breaks are the only structure there is.
pub fn normalize(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A "word" for the purposes below: two or more letters or digits. `Q3`
/// counts; `I`, `||` and `~` do not.
fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|w| w.chars().filter(|c| c.is_alphanumeric()).count() >= 2)
        .count()
}

/// The fewest words worth a chunk of its own. Below this an image is a logo
/// or a decoration, and indexing it costs more in noise than it returns.
const MIN_WORDS: usize = 3;

/// Whether this text is worth indexing.
///
/// An engine pointed at a photo of a wall does not return nothing — it
/// returns a handful of punctuation glyphs it mistook for letters. Those
/// become a chunk, an embedding, and eventually a palette snippet that reads
/// like a bug. Two cheap signals separate a slide from a wall: enough real
/// words, and a majority of the characters actually being letters or digits.
pub fn is_usable(text: &str) -> bool {
    if word_count(text) < MIN_WORDS {
        return false;
    }
    let non_space = text.chars().filter(|c| !c.is_whitespace()).count();
    let alphanumeric = text.chars().filter(|c| c.is_alphanumeric()).count();
    non_space > 0 && alphanumeric * 2 >= non_space
}

/// Split one image's text into passage-sized blocks.
///
/// Most images are one block and stay one chunk. A full-page screenshot is
/// not: left whole it becomes a single oversized chunk whose tail falls off
/// the far side of the embedder's 512-token window, searchable by keyword
/// and invisible to meaning. Splitting on line boundaries keeps every part
/// reachable.
pub fn blocks(text: &str, max_words: usize) -> Vec<String> {
    let max_words = max_words.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut count = 0usize;

    for line in text.lines() {
        let words = line.split_whitespace().count();
        if !current.is_empty() && count + words > max_words {
            out.push(current.join("\n"));
            current.clear();
            count = 0;
        }
        current.push(line);
        count += words;
    }
    if !current.is_empty() {
        out.push(current.join("\n"));
    }
    out
}

/// One line describing an image, for the summary prompt's inventory. The
/// model needs enough to tell one screenshot from another, not the whole
/// slide — the prompt already carries the notes those images sit in.
pub fn for_prompt(text: &str, max_chars: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max_chars {
        return one_line;
    }
    let mut out: String = one_line.chars().take(max_chars).collect();
    // Prefer a word boundary, but not one so early it loses the point.
    if let Some(space) = out.rfind(' ') {
        if space > max_chars / 2 {
            out.truncate(space);
        }
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_spacing_and_keeps_lines() {
        let lines = ["  Q3   revenue ", "", "   ", "4.2M\tvs 3.8M "];
        assert_eq!(normalize(&lines), "Q3 revenue\n4.2M vs 3.8M");
        assert_eq!(normalize(&[]), "");
        assert_eq!(normalize(&["   ", ""]), "");
    }

    #[test]
    fn a_slide_is_usable() {
        assert!(is_usable("Q3 revenue 4.2M\nQ4 forecast 5.1M"));
        assert!(is_usable("Roadmap\nShip the editor\nThen the export"));
    }

    #[test]
    fn a_photo_of_a_wall_is_not() {
        assert!(!is_usable(""));
        assert!(!is_usable("   \n  "));
        // What an engine actually returns from texture and shadow.
        assert!(!is_usable("|| ~ ^^ '"));
        // Enough tokens, but almost none of them are characters.
        assert!(!is_usable("a.b ..... ///// ab cd ef"));
    }

    #[test]
    fn a_word_or_two_is_not_worth_a_chunk() {
        assert!(!is_usable("Roadmap"));
        assert!(!is_usable("ok"));
        assert!(!is_usable("Q3 revenue"));
    }

    #[test]
    fn a_short_image_stays_one_block() {
        let text = "Q3 revenue 4.2M\nQ4 forecast 5.1M";
        assert_eq!(blocks(text, 400), vec![text.to_string()]);
        assert!(blocks("", 400).is_empty());
    }

    #[test]
    fn a_long_image_splits_on_line_boundaries() {
        let line = "one two three four five";
        let text = std::iter::repeat(line).take(10).collect::<Vec<_>>().join("\n");
        // 5 words a line, 10 lines, a 20-word budget → 4 lines per block.
        let out = blocks(&text, 20);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].lines().count(), 4);
        assert_eq!(out[2].lines().count(), 2);
        // Nothing is lost or duplicated in the split.
        assert_eq!(out.join("\n"), text);
    }

    #[test]
    fn one_line_that_alone_exceeds_the_budget_stays_whole() {
        let long = "a b c d e f g h i j k l m n o p";
        assert_eq!(blocks(long, 4), vec![long.to_string()]);
    }

    #[test]
    fn the_prompt_line_is_flattened_and_cut_at_a_word() {
        assert_eq!(for_prompt("Q3\nrevenue  4.2M", 80), "Q3 revenue 4.2M");
        let long = "the quarterly revenue review for the third quarter of the year";
        let cut = for_prompt(long, 20);
        assert!(cut.ends_with('…'), "{cut}");
        assert!(cut.chars().count() <= 21, "{cut}");
        // Cut between words, not through one.
        assert!(long.starts_with(cut.trim_end_matches('…')), "{cut}");
    }
}
