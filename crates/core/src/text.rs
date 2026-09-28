//! The text of files, so searches can match what's inside them.

use std::{fs::File, io::Read, path::Path};

use crate::{Kind, documents, words::words};

/// How much of a file's text is kept in the index unless a scan is told otherwise,
/// in bytes. It covers notes, scripts and most documents whole while keeping logs,
/// data dumps and long books from filling the index.
pub const DEFAULT_TEXT_LIMIT: usize = 64 * 1024;
/// How long a matching line may be before it's shortened.
const LINE_CHARS: usize = 160;
/// Words kept before the first match when a line is shortened.
const LEAD_WORDS: usize = 3;

const PLAIN_EXTS: &[&str] = &["txt", "md", "markdown", "rst", "org", "tex", "csv", "tsv"];

/// Whether files of this kind and extension have text that can be read. Keys and
/// certificates never do.
pub(crate) fn is_readable(kind: Kind, ext: &str) -> bool {
    match kind {
        Kind::Code | Kind::Script | Kind::Config => true,
        Kind::Pdf | Kind::Doc | Kind::Sheet | Kind::Slides => {
            PLAIN_EXTS.contains(&ext) || documents::EXTS.contains(&ext)
        }
        _ => false,
    }
}

/// Up to `limit` bytes of a file's text, or `None` when there is none.
pub(crate) fn read(path: &Path, ext: &str, limit: usize) -> Option<String> {
    if documents::EXTS.contains(&ext) {
        documents::read(path, ext, limit)
    } else {
        read_plain(path, limit)
    }
}

/// The start of a file as text, or `None` when it's empty or not UTF-8.
fn read_plain(path: &Path, limit: usize) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(limit as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.contains(&0) {
        return None;
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // The limit cut a character in half.
        Err(e) if e.utf8_error().error_len().is_none() => {
            let valid = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).ok()?
        }
        Err(_) => return None,
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// The line of `text` with the most of `wanted` in it. A long line is shortened to
/// start just before its first match. `wanted` are lowercase words, matched at the start of
/// words in the line.
pub(crate) fn matching_line(text: &str, wanted: &[String]) -> Option<String> {
    let mut best: Option<(usize, Vec<&str>, usize)> = None;
    for line in text.lines() {
        let pieces: Vec<&str> = line.split_whitespace().collect();
        let mut found = vec![false; wanted.len()];
        let mut first = None;
        for (i, piece) in pieces.iter().enumerate() {
            let piece = words(piece).to_lowercase();
            for (w, word) in wanted.iter().enumerate() {
                if piece
                    .split(' ')
                    .any(|token| token.starts_with(word.as_str()))
                {
                    found[w] = true;
                    first.get_or_insert(i);
                }
            }
        }
        let score = found.iter().filter(|&&f| f).count();
        if let Some(first) = first
            && best.as_ref().is_none_or(|(best, _, _)| score > *best)
        {
            best = Some((score, pieces, first));
        }
    }
    let (_, pieces, first) = best?;
    let whole = pieces.join(" ");
    let start = if whole.chars().count() > LINE_CHARS {
        first.saturating_sub(LEAD_WORDS)
    } else {
        0
    };
    let mut line = pieces[start..].join(" ");
    if start > 0 {
        line.insert_str(0, "… ");
    }
    if let Some((cut, _)) = line.char_indices().nth(LINE_CHARS) {
        line.truncate(cut);
        line.push('…');
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::matching_line;

    fn wanted(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn picks_the_line_with_the_most_matches() {
        let text = "#!/bin/sh\n# copy photos\nrsync -av ~/Pictures nas:/photos\n";
        assert_eq!(
            matching_line(text, &wanted(&["rsync", "photo"])).as_deref(),
            Some("rsync -av ~/Pictures nas:/photos")
        );
        assert_eq!(
            matching_line(text, &wanted(&["copy"])).as_deref(),
            Some("# copy photos")
        );
        assert_eq!(matching_line(text, &wanted(&["tar"])), None);
    }

    #[test]
    fn matches_the_start_of_words_inside_pieces() {
        let text = "call backupPhotos now";
        assert_eq!(
            matching_line(text, &wanted(&["phot"])).as_deref(),
            Some("call backupPhotos now")
        );
    }

    #[test]
    fn shortens_long_lines() {
        let text = format!("{} needle {}", "hay ".repeat(20), "straw ".repeat(60));
        let line = matching_line(&text, &wanted(&["needle"])).unwrap();
        assert!(line.starts_with("… hay hay hay needle straw"));
        assert!(line.ends_with('…'));
        assert_eq!(line.chars().count(), 161);
    }

    #[test]
    fn matches_cjk_characters() {
        assert_eq!(
            matching_line("备注\n报销发票 三月", &wanted(&["报", "销"])).as_deref(),
            Some("报销发票 三月")
        );
    }
}
