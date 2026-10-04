//! 从宿主读回的文本里找回原句范围。光标只是提示，匹配到的文本才是依据。

use crate::text::{slice_utf16, utf16_len};

use super::Utf16Range;

/// 光标前、长度等于原文的范围。未知光标不能退回文档开头，否则忽略范围的宿主会追加。
pub fn range_behind_caret(
    original_utf16_length: usize,
    selected: Option<Utf16Range>,
) -> Option<Utf16Range> {
    let length = original_utf16_length;
    let selected = selected?;
    if length == 0 {
        return None;
    }
    if selected.length == length {
        return Some(selected);
    }
    let caret = if selected.length == 0 {
        selected.location
    } else {
        selected.end()
    };
    if caret < length {
        return None;
    }
    Some(Utf16Range::new(caret - length, length))
}

/// 在光标附近向后读一段，找原文最后一次出现的位置。
///
/// 有的宿主光标落后句末标点一个字符，所以窗口会多读一个代码单元。宿主完全读不到文本时返回 `None`，
/// 调用方必须改走只依赖光标的路径，不能假装找到了范围。
pub fn range_searching_backwards(
    original: &str,
    selected: Option<Utf16Range>,
    window_length: usize,
    mut read_text: impl FnMut(Utf16Range) -> Option<String>,
) -> Option<Utf16Range> {
    let length = utf16_len(original);
    let selected = selected?;
    if length == 0 || window_length == 0 {
        return None;
    }
    let lookbehind = selected.location.min(window_length);
    let start = selected.location - lookbehind;
    let window = read_text(Utf16Range::new(start, lookbehind + 1))
        .or_else(|| read_text(Utf16Range::new(start, lookbehind)))?;
    let window_len = utf16_len(&window);
    if window_len == 0 {
        return None;
    }
    let caret = (selected.location - start).min(window_len);
    let upper = window_len.min(caret + 1);
    if upper < length {
        return None;
    }
    let search = slice_utf16(&window, 0, upper)?;
    let found = last_utf16_match(search, original)?;
    Some(Utf16Range::new(start + found, length))
}

/// 优先用读回来确实等于原文的范围。微信会把光标报在句子后一个字符，精确恢复的范围必须赢。
pub fn preferred_range(
    original: &str,
    candidates: &[Option<Utf16Range>],
    mut read_text: impl FnMut(Utf16Range) -> Option<String>,
) -> Option<Utf16Range> {
    let expected = utf16_len(original);
    let usable = candidates
        .iter()
        .copied()
        .flatten()
        .filter(|range| range.length == expected);
    let mut first = None;
    for range in usable {
        if first.is_none() {
            first = Some(range);
        }
        if read_text(range).as_deref() == Some(original) {
            return Some(range);
        }
    }
    first
}

/// 在整段可读文档里找光标附近的原文。光标落后标点时，允许匹配终点比光标多一个代码单元。
pub fn range_of_original(
    document: Option<&str>,
    document_offset: usize,
    selected: Option<Utf16Range>,
    original: &str,
) -> Option<Utf16Range> {
    let document = document?;
    let selected = selected?;
    let length = utf16_len(original);
    if length == 0 || selected.location < document_offset {
        return None;
    }
    let local = selected.location - document_offset;
    let document_len = utf16_len(document);
    if local > document_len {
        return None;
    }
    let upper = document_len.min(local + 1);
    if upper < length {
        return None;
    }
    let search = slice_utf16(document, 0, upper)?;
    let found = last_utf16_match(search, original)?;
    Some(Utf16Range::new(document_offset + found, length))
}

/// 当前行里、光标前的整句。不会跨过换行。
pub fn sentence_on_current_line(
    document: Option<&str>,
    selected: Option<Utf16Range>,
) -> Option<(String, Utf16Range)> {
    let document = document?;
    let selected = selected?;
    if selected.length != 0 {
        return None;
    }
    let document_len = utf16_len(document);
    if selected.location > document_len {
        return None;
    }
    let line = line_bounds(document, selected.location.min(document_len))?;
    let end = selected.location.min(line.end);
    if end < line.start {
        return None;
    }
    let raw = slice_utf16(document, line.start, end - line.start)?;
    let leading = raw
        .chars()
        .take_while(|character| character.is_whitespace())
        .collect::<String>();
    let text = raw.trim().to_owned();
    if text.is_empty() {
        return None;
    }
    let location = line.start + utf16_len(&leading);
    let length = utf16_len(&text);
    Some((text, Utf16Range::new(location, length)))
}

/// 恢复粘贴或在 Lucid 开始跟踪之前已经存在的上下文。
/// 已跟踪的尾巴必须就在光标处，只把同一行里它前面的文字加进来。
pub fn sentence_before_cursor(
    document: Option<&str>,
    selected: Option<Utf16Range>,
    tracked: &str,
) -> Option<(String, Utf16Range)> {
    let document = document?;
    let selected = selected?;
    if selected.length != 0 {
        return None;
    }
    let document_len = utf16_len(document);
    if selected.location > document_len {
        return None;
    }
    let tracked_length = utf16_len(tracked);
    if tracked_length == 0 {
        return None;
    }
    let cursor_after = document_len.min(selected.location + 1);
    let mut tracked_start = None;
    for end in [selected.location, cursor_after] {
        if end < tracked_length {
            continue;
        }
        let start = end - tracked_length;
        if slice_utf16(document, start, tracked_length) == Some(tracked) {
            tracked_start = Some(start);
            break;
        }
    }
    let tracked_start = tracked_start?;
    let line = line_bounds(document, tracked_start)?;
    let context_end = tracked_start + tracked_length;
    if context_end < line.start || context_end - line.start < tracked_length {
        return None;
    }
    let context = slice_utf16(document, line.start, context_end - line.start)?;
    let leading = context
        .chars()
        .take_while(|character| character.is_whitespace())
        .collect::<String>();
    let text = context.trim().to_owned();
    if text.is_empty() {
        return None;
    }
    let length = utf16_len(&text);
    Some((
        text,
        Utf16Range::new(line.start + utf16_len(&leading), length),
    ))
}

struct LineBounds {
    start: usize,
    end: usize,
}

fn line_bounds(document: &str, location: usize) -> Option<LineBounds> {
    let document_len = utf16_len(document);
    let location = location.min(document_len);
    let mut start = 0;
    let mut seen = 0;
    for character in document.chars() {
        if seen >= location {
            break;
        }
        let width = character.len_utf16();
        if character == '\n' || character == '\r' {
            start = seen + width;
        }
        seen += width;
    }
    let mut end = start;
    let mut seen = 0;
    let mut inside = false;
    for character in document.chars() {
        let width = character.len_utf16();
        if seen >= start {
            inside = true;
        }
        if inside && (character == '\n' || character == '\r') {
            break;
        }
        seen += width;
        if inside {
            end = seen;
        }
    }
    if !inside && start == document_len {
        end = document_len;
    }
    Some(LineBounds { start, end })
}

fn last_utf16_match(haystack: &str, needle: &str) -> Option<usize> {
    let needle_len = utf16_len(needle);
    let haystack_len = utf16_len(haystack);
    if needle_len == 0 || haystack_len < needle_len {
        return None;
    }
    let mut offset = haystack_len - needle_len;
    loop {
        if slice_utf16(haystack, offset, needle_len) == Some(needle) {
            return Some(offset);
        }
        if offset == 0 {
            return None;
        }
        offset -= 1;
    }
}
