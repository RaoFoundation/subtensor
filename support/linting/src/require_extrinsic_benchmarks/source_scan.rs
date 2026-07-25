//! Mask comments/strings and locate identifiers/braces in pallet source text.
//!
//! These helpers underwrite the dispatchable and benchmark scanners: both walk
//! comment-masked source so string literals and `//` / `/* */` cannot fake a match.

pub(super) fn mask_comments_and_strings(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut idx = 0;

    while idx < bytes.len() {
        match (bytes[idx], bytes.get(idx + 1).copied()) {
            (b'/', Some(b'/')) => {
                out.push(' ');
                out.push(' ');
                idx += 2;
                while idx < bytes.len() && bytes[idx] != b'\n' {
                    out.push(' ');
                    idx += 1;
                }
            }
            (b'/', Some(b'*')) => {
                out.push(' ');
                out.push(' ');
                idx += 2;
                let mut depth = 1usize;
                while idx < bytes.len() && depth > 0 {
                    if bytes[idx] == b'\n' {
                        out.push('\n');
                        idx += 1;
                    } else if bytes[idx] == b'/' && bytes.get(idx + 1) == Some(&b'*') {
                        out.push(' ');
                        out.push(' ');
                        idx += 2;
                        depth += 1;
                    } else if bytes[idx] == b'*' && bytes.get(idx + 1) == Some(&b'/') {
                        out.push(' ');
                        out.push(' ');
                        idx += 2;
                        depth -= 1;
                    } else {
                        out.push(' ');
                        idx += 1;
                    }
                }
            }
            (b'"', _) => {
                out.push(' ');
                idx += 1;
                let mut escaped = false;
                while idx < bytes.len() {
                    let byte = bytes[idx];
                    if byte == b'\n' {
                        out.push('\n');
                        idx += 1;
                        break;
                    }
                    out.push(' ');
                    idx += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        break;
                    }
                }
            }
            (b'\'', _) if !bytes.get(idx + 1).is_some_and(|byte| is_ident_start(*byte)) => {
                out.push(' ');
                idx += 1;
                let mut escaped = false;
                while idx < bytes.len() {
                    let byte = bytes[idx];
                    if byte == b'\n' {
                        out.push('\n');
                        idx += 1;
                        break;
                    }
                    out.push(' ');
                    idx += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'\'' {
                        break;
                    }
                }
            }
            (byte, _) => {
                out.push(byte as char);
                idx += 1;
            }
        }
    }

    out
}

pub(super) fn find_matching_brace(masked: &str, open_brace: usize) -> Option<usize> {
    find_matching_delimiter(masked, open_brace, b'{', b'}')
}

pub(super) fn find_matching_paren(masked: &str, open_paren: usize) -> Option<usize> {
    find_matching_delimiter(masked, open_paren, b'(', b')')
}

pub(super) fn find_matching_delimiter(
    masked: &str,
    open: usize,
    open_byte: u8,
    close_byte: u8,
) -> Option<usize> {
    let bytes = masked.as_bytes();
    if bytes.get(open) != Some(&open_byte) {
        return None;
    }

    let mut depth = 0usize;
    for (idx, byte) in bytes.iter().enumerate().skip(open) {
        if *byte == open_byte {
            depth += 1;
        } else if *byte == close_byte {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }

    None
}

pub(super) fn find_word(masked: &str, word: &str, from: usize) -> Option<usize> {
    let mut search_from = from;
    while let Some(offset) = masked[search_from..].find(word) {
        let idx = search_from + offset;
        if starts_with_word(masked, idx, word) {
            return Some(idx);
        }
        search_from = idx + word.len();
    }

    None
}

pub(super) fn starts_with_word(masked: &str, idx: usize, word: &str) -> bool {
    let bytes = masked.as_bytes();
    let word_bytes = word.as_bytes();

    if bytes.get(idx..idx + word_bytes.len()) != Some(word_bytes) {
        return false;
    }

    let before_ok = idx == 0 || !is_ident_continue(bytes[idx - 1]);
    let after_idx = idx + word_bytes.len();
    let after_ok = after_idx >= bytes.len() || !is_ident_continue(bytes[after_idx]);
    before_ok && after_ok
}

pub(super) fn parse_ident(masked: &str, start: usize) -> Option<(String, usize)> {
    let bytes = masked.as_bytes();
    let first = *bytes.get(start)?;
    if !is_ident_start(first) {
        return None;
    }

    let mut end = start + 1;
    while bytes.get(end).is_some_and(|byte| is_ident_continue(*byte)) {
        end += 1;
    }

    Some((masked[start..end].to_owned(), end))
}

pub(super) fn skip_ws(masked: &str, mut idx: usize) -> usize {
    let bytes = masked.as_bytes();
    while bytes
        .get(idx)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        idx += 1;
    }
    idx
}

pub(super) fn rtrim_ws(masked: &str, start: usize, mut end: usize) -> Option<usize> {
    let bytes = masked.as_bytes();
    while end > start
        && bytes
            .get(end - 1)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        end -= 1;
    }
    end.checked_sub(1).filter(|idx| *idx >= start)
}

pub(super) fn end_of_line(masked: &str, from: usize) -> usize {
    masked[from..]
        .find('\n')
        .map(|offset| from + offset)
        .unwrap_or(masked.len())
}

pub(super) fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

pub(super) fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

pub(super) fn line_column(source: &str, idx: usize) -> (usize, usize) {
    let line = source[..idx].bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = source[..idx]
        .rfind('\n')
        .map(|line_start| idx - line_start)
        .unwrap_or(idx + 1);
    (line, column)
}
