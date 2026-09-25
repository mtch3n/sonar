pub(crate) fn words(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if is_cjk(c) {
            out.push(' ');
            out.push(c);
            out.push(' ');
            prev = None;
        } else if c.is_alphanumeric() {
            if let Some(p) = prev {
                let camel = p.is_lowercase() && c.is_uppercase();
                let digit_edge = p.is_numeric() != c.is_numeric();
                if camel || digit_edge {
                    out.push(' ');
                }
            }
            out.push(c);
            prev = Some(c);
        } else {
            out.push(' ');
            prev = None;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x3040..=0x30FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF
        | 0x20000..=0x2FA1F
    )
}

#[cfg(test)]
mod tests {
    use super::words;

    #[test]
    fn splits_camel_case_digits_and_punctuation() {
        assert_eq!(words("myBackupScript_v2.sh"), "my Backup Script v 2 sh");
        assert_eq!(words("invoice-2024-03.pdf"), "invoice 2024 03 pdf");
    }

    #[test]
    fn isolates_cjk_characters() {
        assert_eq!(words("报销发票2024.pdf"), "报 销 发 票 2024 pdf");
    }
}
