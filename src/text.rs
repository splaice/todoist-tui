use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn fit(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if display_width(s) <= width {
        return s.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut col = 0;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + w > width - 1 {
            break;
        }
        out.push(ch);
        col += w;
    }
    out.push('…');
    out
}

pub fn slice_cols(s: &str, start: usize, end: usize) -> String {
    let mut col = 0;
    let mut out = String::new();
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col >= end {
            break;
        }
        if col >= start {
            out.push(ch);
        }
        col += w;
    }
    out
}

/// Split `s` into display-width chunks. A newline starts a new row, including blank lines.
pub fn wrap_parts(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    for line in s.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        wrap_line(line, width, &mut out);
    }
    out
}

fn wrap_line(s: &str, width: usize, out: &mut Vec<String>) {
    let mut rest = s;
    while display_width(rest) > width {
        let mut end = 0;
        let mut col = 0;
        let mut soft = None;
        for (i, ch) in rest.char_indices() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w == 0 {
                continue;
            }
            if col + w > width {
                break;
            }
            end = i + ch.len_utf8();
            col += w;
            if ch.is_whitespace() {
                soft = Some(end);
            }
        }
        let mut cut = end;
        if let Some(soft_at) = soft {
            let soft_width = display_width(&rest[..soft_at]);
            if soft_width >= width / 2 {
                cut = soft_at;
            }
        }
        if cut == 0 {
            if let Some(ch) = rest.chars().next() {
                cut = ch.len_utf8();
            } else {
                break;
            }
        }
        out.push(rest[..cut].trim_end().to_string());
        rest = rest[cut..].trim_start();
        if rest.is_empty() {
            break;
        }
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_adds_an_ellipsis_when_the_text_is_wider_than_the_column() {
        assert_eq!(fit("Personal Health", 8), "Persona…");
        assert_eq!(fit("Inbox", 8), "Inbox");
        assert_eq!(fit("ab", 0), "");
    }

    #[test]
    fn wrap_parts_keeps_newlines_as_separate_rows() {
        assert_eq!(
            wrap_parts("Coverage.\nCURRENT: Platinum", 40),
            vec!["Coverage.".to_string(), "CURRENT: Platinum".to_string()]
        );
        assert_eq!(
            wrap_parts("a\r\n\r\nb", 40),
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
        assert_eq!(
            wrap_parts("alpha beta gamma", 10),
            vec!["alpha".to_string(), "beta gamma".to_string()]
        );
    }
}
