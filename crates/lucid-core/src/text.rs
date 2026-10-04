//! UTF-16 文本工具。输入法宿主按 UTF-16 计范围，Core 不能按 Rust 字符数换算。

/// 与 `NSString.length` 一致的 UTF-16 代码单元数。
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// 不区分大小写比较，长度必须相同。
///
/// 备忘录和多数网页会把刚输入的句首字母自动大写。记录下来的原文和宿主读回的
/// 文本字节不同，但还是同一句，不能因此拒绝替换。
pub fn equal_ignore_case(left: &str, right: &str) -> bool {
    if utf16_len(left) != utf16_len(right) {
        return false;
    }
    left.chars()
        .flat_map(char::to_lowercase)
        .eq(right.chars().flat_map(char::to_lowercase))
}

/// 去掉首尾空白后，是否以句末标点结束。
pub fn ends_with_terminator(text: &str) -> bool {
    text.trim().chars().next_back().is_some_and(is_terminator)
}

pub fn is_terminator(character: char) -> bool {
    ".!?。？！".contains(character)
}

/// 取以 UTF-16 偏移和长度描述的子串。越界返回 `None`。
pub fn slice_utf16(text: &str, location: usize, length: usize) -> Option<&str> {
    if length == 0 {
        let start = utf16_index(text, location)?;
        return Some(&text[start..start]);
    }
    let start = utf16_index(text, location)?;
    let end = utf16_index(text, location + length)?;
    Some(&text[start..end])
}

fn utf16_index(text: &str, units: usize) -> Option<usize> {
    if units == 0 {
        return Some(0);
    }
    let mut seen = 0;
    for (index, character) in text.char_indices() {
        if seen == units {
            return Some(index);
        }
        seen += character.len_utf16();
        if seen > units {
            return None;
        }
    }
    (seen == units).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use super::{equal_ignore_case, slice_utf16, utf16_len};

    #[test]
    fn emoji_counts_as_two_utf16_units() {
        assert_eq!(utf16_len("a😀b"), 4);
        assert_eq!(slice_utf16("a😀b", 1, 2), Some("😀"));
    }

    #[test]
    fn case_fold_requires_same_length() {
        assert!(equal_ignore_case("Nihao?", "nihao?"));
        assert!(!equal_ignore_case("hi", "high"));
    }
}
