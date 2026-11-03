/// 字數統計：中日韓文字每字算一，其他語言以連續的字母數字算一個詞。標點不計。
pub fn count_words(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for c in text.chars() {
        if is_cjk(c) {
            count += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else if (c == '\'' || c == '’' || c == '-') && in_word {
            // don't, well-known 視為同一個詞
        } else {
            in_word = false;
        }
    }
    count
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF     // 平假名、片假名
        | 0x3400..=0x4DBF   // CJK 擴展 A
        | 0x4E00..=0x9FFF   // CJK 基本
        | 0xAC00..=0xD7AF   // 諺文
        | 0xF900..=0xFAFF   // CJK 相容
        | 0x20000..=0x3134F // CJK 擴展 B–G
    )
}

/// 回傳游標（字元索引）所在段落的位元組範圍。
pub fn paragraph_range(text: &str, char_index: usize) -> (usize, usize) {
    let byte = text
        .char_indices()
        .nth(char_index)
        .map(|(b, _)| b)
        .unwrap_or(text.len());
    let start = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[byte..].find('\n').map(|i| byte + i).unwrap_or(text.len());
    (start, end)
}

pub fn fmt_duration(secs: f64) -> String {
    let s = secs.max(0.0).ceil() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_mixed_text() {
        assert_eq!(count_words("今天天氣很好。"), 6);
        assert_eq!(count_words("Hello, world!"), 2);
        assert_eq!(count_words("我用 Rust 寫了 2 個程式"), 9);
        assert_eq!(count_words("don't stop"), 2);
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn finds_paragraph() {
        let t = "第一段\n第二段\n第三段";
        assert_eq!(&t[{ let (a, b) = paragraph_range(t, 5); a..b }], "第二段");
        assert_eq!(&t[{ let (a, b) = paragraph_range(t, 0); a..b }], "第一段");
        assert_eq!(&t[{ let (a, b) = paragraph_range(t, 99); a..b }], "第三段");
    }
}
