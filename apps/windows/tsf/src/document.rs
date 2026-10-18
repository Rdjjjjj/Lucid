//! 通过一次同步编辑会话读写当前 TSF 文档。
//!
//! 读不回原文就不写。失败后不再尝试第二次写入。

use std::cell::Cell;

use windows::{
    Win32::UI::TextServices::{
        ITfContext, ITfEditSession, ITfEditSession_Impl, ITfInsertAtSelection, ITfRange,
        ITfRangeACP, TF_AE_NONE, TF_ANCHOR_END, TF_DEFAULT_SELECTION, TF_ES_READ, TF_ES_READWRITE,
        TF_ES_SYNC, TF_IAS_NOQUERY, TF_SELECTION, TF_SELECTIONSTYLE,
    },
    core::{Interface, Result, implement},
};

use lucid_core::{ReplacementOutcome, Utf16Range};

thread_local! {
    static EDIT_RESULT: Cell<Option<EditResult>> = const { Cell::new(None) };
}

#[derive(Clone, Debug)]
pub enum EditResult {
    Empty,
    Caret(i32),
    Replaced,
    Stale,
    Failed,
}

#[derive(Clone, Debug)]
pub enum EditCommand {
    Insert(String),
    DeleteBackward,
    ReadCaret,
    Replace {
        anchor: i32,
        length: i32,
        text: String,
        expected: String,
    },
}

pub fn run(context: &ITfContext, client_id: u32, command: EditCommand) -> Result<EditResult> {
    let write = !matches!(command, EditCommand::ReadCaret);
    let flags = if write {
        TF_ES_SYNC | TF_ES_READWRITE
    } else {
        TF_ES_SYNC | TF_ES_READ
    };
    EDIT_RESULT.set(None);
    let session: ITfEditSession = DocumentEdit { command }.into();
    let status = unsafe { context.RequestEditSession(client_id, &session, flags)? };
    status.ok()?;
    Ok(EDIT_RESULT.take().unwrap_or(EditResult::Failed))
}

pub fn insert(context: &ITfContext, client_id: u32, text: &str) -> Result<()> {
    match run(context, client_id, EditCommand::Insert(text.to_owned()))? {
        EditResult::Failed => Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            -2147467259,
        ))),
        _ => Ok(()),
    }
}

pub fn delete_backward(context: &ITfContext, client_id: u32) -> Result<()> {
    match run(context, client_id, EditCommand::DeleteBackward)? {
        EditResult::Failed => Err(windows::core::Error::from_hresult(windows::core::HRESULT(
            -2147467259,
        ))),
        _ => Ok(()),
    }
}

pub fn caret(context: &ITfContext, client_id: u32) -> Option<usize> {
    match run(context, client_id, EditCommand::ReadCaret).ok()? {
        EditResult::Caret(value) if value >= 0 => Some(value as usize),
        _ => None,
    }
}

/// 在同一次编辑会话里读回、替换、再读回。读不回原文就不写。
pub fn replace_validated(
    context: &ITfContext,
    client_id: u32,
    original: &str,
    replacement: &str,
    range: Utf16Range,
) -> ReplacementOutcome {
    if original.is_empty() || replacement.is_empty() || range.length == 0 {
        return ReplacementOutcome::Stale;
    }
    let Ok(anchor) = i32::try_from(range.location) else {
        return ReplacementOutcome::Stale;
    };
    let Ok(length) = i32::try_from(range.length) else {
        return ReplacementOutcome::Stale;
    };
    match run(
        context,
        client_id,
        EditCommand::Replace {
            anchor,
            length,
            text: replacement.to_owned(),
            expected: original.to_owned(),
        },
    ) {
        Ok(EditResult::Replaced) => ReplacementOutcome::Replaced,
        Ok(EditResult::Stale) => ReplacementOutcome::Stale,
        _ => ReplacementOutcome::Unverified,
    }
}

#[implement(ITfEditSession)]
struct DocumentEdit {
    command: EditCommand,
}

impl ITfEditSession_Impl for DocumentEdit_Impl {
    fn DoEditSession(&self, cookie: u32) -> Result<()> {
        let context = current_context();
        let Some(context) = context else {
            EDIT_RESULT.set(Some(EditResult::Failed));
            return Ok(());
        };
        let result = match &self.command {
            EditCommand::Insert(text) => insert_at_selection(&context, cookie, text),
            EditCommand::DeleteBackward => delete_before_caret(&context, cookie),
            EditCommand::ReadCaret => read_caret(&context, cookie),
            EditCommand::Replace {
                anchor,
                length,
                text,
                expected,
            } => replace_span(&context, cookie, *anchor, *length, text, expected),
        };
        EDIT_RESULT.set(Some(result.unwrap_or(EditResult::Failed)));
        Ok(())
    }
}

thread_local! {
    static CURRENT_CONTEXT: Cell<Option<ITfContext>> = const { Cell::new(None) };
}

pub fn with_context<T>(context: &ITfContext, body: impl FnOnce() -> T) -> T {
    CURRENT_CONTEXT.set(Some(context.clone()));
    let value = body();
    CURRENT_CONTEXT.set(None);
    value
}

fn current_context() -> Option<ITfContext> {
    CURRENT_CONTEXT.take()
}

fn insert_at_selection(context: &ITfContext, cookie: u32, text: &str) -> Result<EditResult> {
    let insert: ITfInsertAtSelection = context.cast()?;
    let units = utf16(text);
    unsafe { insert.InsertTextAtSelection(cookie, TF_IAS_NOQUERY, &units)? };
    Ok(EditResult::Empty)
}

fn delete_before_caret(context: &ITfContext, cookie: u32) -> Result<EditResult> {
    let selection = selection(context, cookie)?;
    let range = selection.range.as_ref().ok_or_else(failed)?;
    let mut moved = 0;
    unsafe { range.ShiftStart(cookie, -1, &mut moved, std::ptr::null())? };
    if moved == 0 {
        return Ok(EditResult::Failed);
    }
    unsafe { range.SetText(cookie, 0, &[])? };
    Ok(EditResult::Empty)
}

fn read_caret(context: &ITfContext, cookie: u32) -> Result<EditResult> {
    let selection = selection(context, cookie)?;
    let range = selection.range.as_ref().ok_or_else(failed)?;
    let acp: ITfRangeACP = range.cast()?;
    let mut anchor = 0;
    let mut length = 0;
    unsafe { acp.GetExtent(&mut anchor, &mut length)? };
    Ok(EditResult::Caret(anchor))
}

fn replace_span(
    context: &ITfContext,
    cookie: u32,
    anchor: i32,
    length: i32,
    text: &str,
    expected: &str,
) -> Result<EditResult> {
    let range = range_at(context, cookie, anchor, length)?;
    let current = range_text(&range, cookie)?;
    if !equal_ignore_case(&current, expected) {
        return Ok(EditResult::Stale);
    }
    select(context, cookie, &range)?;
    unsafe { range.SetText(cookie, 0, &utf16(text))? };
    let written = range_text(&range, cookie)?;
    if written != text {
        return Ok(EditResult::Failed);
    }
    Ok(EditResult::Replaced)
}

fn equal_ignore_case(left: &str, right: &str) -> bool {
    left.chars()
        .zip(right.chars())
        .all(|(left, right)| left.eq_ignore_ascii_case(&right))
        && left.chars().count() == right.chars().count()
}

fn selection(context: &ITfContext, cookie: u32) -> Result<TF_SELECTION> {
    let mut fetched = 0;
    let mut selections = [TF_SELECTION {
        range: std::mem::ManuallyDrop::new(None),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: false.into(),
        },
    }];
    unsafe {
        context.GetSelection(cookie, TF_DEFAULT_SELECTION, &mut selections, &mut fetched)?;
    }
    if fetched == 0 {
        return Err(failed());
    }
    let range = unsafe { std::mem::ManuallyDrop::take(&mut selections[0].range) };
    Ok(TF_SELECTION {
        range: std::mem::ManuallyDrop::new(range),
        style: selections[0].style,
    })
}

fn range_at(context: &ITfContext, cookie: u32, anchor: i32, length: i32) -> Result<ITfRange> {
    let start = unsafe { context.GetStart(cookie)? };
    let mut moved = 0;
    unsafe { start.ShiftEnd(cookie, anchor, &mut moved, std::ptr::null())? };
    unsafe { start.Collapse(cookie, TF_ANCHOR_END)? };
    unsafe { start.ShiftEnd(cookie, length, &mut moved, std::ptr::null())? };
    Ok(start)
}

fn range_text(range: &ITfRange, cookie: u32) -> Result<String> {
    let mut buffer = vec![0u16; 4096];
    let mut copied = 0;
    unsafe { range.GetText(cookie, 0, &mut buffer, &mut copied)? };
    let copied = usize::try_from(copied).unwrap_or(0).min(buffer.len());
    Ok(String::from_utf16_lossy(&buffer[..copied]))
}

fn select(context: &ITfContext, cookie: u32, range: &ITfRange) -> Result<()> {
    let selection = [TF_SELECTION {
        range: std::mem::ManuallyDrop::new(Some(range.clone())),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: false.into(),
        },
    }];
    unsafe { context.SetSelection(cookie, &selection)? };
    Ok(())
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn failed() -> windows::core::Error {
    windows::core::Error::from_hresult(windows::core::HRESULT(-2147467259))
}
