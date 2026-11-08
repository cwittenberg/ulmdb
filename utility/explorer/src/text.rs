//! Database bytes and filenames must never become terminal control sequences.
use std::fmt::Write;

pub fn escaped(bytes: &[u8]) -> String {
    let mut output = String::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                safe_text(text, &mut output);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if let Ok(text) = std::str::from_utf8(&remaining[..valid]) {
                    safe_text(text, &mut output);
                }
                let invalid = error.error_len().unwrap_or(remaining.len() - valid);
                for byte in &remaining[valid..valid + invalid] {
                    let _ = write!(output, "\\x{byte:02x}");
                }
                remaining = &remaining[valid + invalid..];
            }
        }
    }
    output
}

fn safe_text(text: &str, output: &mut String) {
    for character in text.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control()
                || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
            {
                let _ = write!(output, "\\u{{{:x}}}", ch as u32);
            }
            ch => output.push(ch),
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(3));
    for (index, byte) in bytes.iter().enumerate() {
        if index != 0 {
            output.push(' ');
        }
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_controls_and_bidi_are_visible_without_losing_unicode() {
        let shown = escaped(b"hello\x1b[2J\n\\\xff\0\xf0\x9f\x8c\xb1");
        assert_eq!(shown, "hello\\u{1b}[2J\\n\\\\\\xff\\u{0}🌱");
        assert_eq!(escaped("a\u{202e}b".as_bytes()), "a\\u{202e}b");
        assert_eq!(hex(&[0, 255, 32]), "00 ff 20");
    }
}
