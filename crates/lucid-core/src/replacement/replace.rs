//! 把英文写回已经提交的原文范围，并读回校验。
//!
//! 一次操作后读回验证，不对未知结果继续写入。已知忽略范围的宿主必须由
//! 平台层选择经过验证的选区替换路径，不能尝试普通范围插入。

use crate::text::{equal_ignore_case, utf16_len};

use super::Utf16Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementOutcome {
    Replaced,
    Stale,
    Unverified,
    /// No supported write path was attempted; the original was not changed.
    Unsupported,
}

/// 宿主能力的最小视图。测试用内存文档实现它，macOS 壳用 IMK 实现它。
pub struct HostClient<R, S, M, K, I, D, L>
where
    R: FnMut(Utf16Range) -> Option<String>,
    S: FnMut() -> Option<Utf16Range>,
    M: FnMut(&str, Utf16Range, Utf16Range),
    K: FnMut() -> Option<Utf16Range>,
    I: FnMut(&str, Utf16Range),
    D: FnMut(),
    L: FnMut(Utf16Range),
{
    pub read_text: R,
    pub selected_range: S,
    pub set_marked_text: Option<M>,
    pub marked_range: Option<K>,
    pub insert_text: I,
    pub delete_backward: Option<D>,
    pub set_selection: Option<L>,
}

pub struct CommittedTextReplacement;

impl CommittedTextReplacement {
    pub fn matches_loosely(read: Option<&str>, original: &str) -> bool {
        read.is_some_and(|value| equal_ignore_case(value, original))
    }

    pub fn replace_range<R, S, M, K, I, D, L>(
        original: &str,
        replacement: &str,
        range: Utf16Range,
        client: &mut HostClient<R, S, M, K, I, D, L>,
    ) -> ReplacementOutcome
    where
        R: FnMut(Utf16Range) -> Option<String>,
        S: FnMut() -> Option<Utf16Range>,
        M: FnMut(&str, Utf16Range, Utf16Range),
        K: FnMut() -> Option<Utf16Range>,
        I: FnMut(&str, Utf16Range),
        D: FnMut(),
        L: FnMut(Utf16Range),
    {
        if !valid_attempt(original, replacement, range) {
            return ReplacementOutcome::Stale;
        }
        if !Self::matches_loosely((client.read_text)(range).as_deref(), original) {
            return ReplacementOutcome::Stale;
        }
        // A selected-text insertion is safe only after reading back the exact
        // selection. Never delete character-by-character: a host can stop
        // halfway through and leave the user's original damaged.
        if let Some(select) = client.set_selection.as_mut() {
            select(range);
        }
        let target = if (client.selected_range)() == Some(range) {
            Utf16Range::new(usize::MAX, 0)
        } else {
            range
        };
        (client.insert_text)(replacement, target);
        if replacement_landed(replacement, range, Some(original), client) {
            ReplacementOutcome::Replaced
        } else {
            // A failed readback is not a licence to write again. In particular,
            // insertText("", range) cannot undo an append in a range-ignoring
            // host, and setMarkedText would append a second copy there.
            ReplacementOutcome::Unverified
        }
    }
}

fn valid_attempt(original: &str, replacement: &str, range: Utf16Range) -> bool {
    !original.is_empty()
        && !replacement.is_empty()
        && range.length == utf16_len(original)
        && range.location.checked_add(range.length).is_some()
        && range.location.checked_add(utf16_len(replacement)).is_some()
}

fn replacement_landed<R, S, M, K, I, D, L>(
    replacement: &str,
    range: Utf16Range,
    original: Option<&str>,
    client: &mut HostClient<R, S, M, K, I, D, L>,
) -> bool
where
    R: FnMut(Utf16Range) -> Option<String>,
    S: FnMut() -> Option<Utf16Range>,
    M: FnMut(&str, Utf16Range, Utf16Range),
    K: FnMut() -> Option<Utf16Range>,
    I: FnMut(&str, Utf16Range),
    D: FnMut(),
    L: FnMut(Utf16Range),
{
    let expected = Utf16Range::new(range.location, utf16_len(replacement));
    if (client.read_text)(expected).as_deref() != Some(replacement) {
        return false;
    }
    if let Some(original) = original {
        let leftover = Utf16Range::new(expected.end(), utf16_len(original));
        if (client.read_text)(leftover).as_deref() == Some(original) {
            return false;
        }
    }
    true
}
