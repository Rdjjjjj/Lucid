use std::cell::RefCell;
use std::rc::Rc;

use lucid_core::replacement::{
    preferred_range, range_behind_caret, range_of_original, range_searching_backwards,
    sentence_before_cursor, sentence_on_current_line,
};
use lucid_core::{CommittedTextReplacement, HostClient, ReplacementOutcome, Utf16Range};

struct Document {
    text: String,
    caret: usize,
    marked: Option<Utf16Range>,
}

impl Document {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            caret: text.encode_utf16().count(),
            marked: None,
        }
    }

    fn len(&self) -> usize {
        self.text.encode_utf16().count()
    }

    fn read(&self, range: Utf16Range) -> Option<String> {
        if range.location == usize::MAX || range.location + range.length > self.len() {
            return None;
        }
        let start = utf16_byte(&self.text, range.location)?;
        let end = utf16_byte(&self.text, range.location + range.length)?;
        Some(self.text[start..end].to_owned())
    }

    fn replace_range(&mut self, range: Utf16Range, text: &str) {
        let Some(start) = utf16_byte(&self.text, range.location) else {
            return;
        };
        let Some(end) = utf16_byte(&self.text, range.location + range.length) else {
            return;
        };
        self.text.replace_range(start..end, text);
    }
}

fn client(
    document: &Rc<RefCell<Document>>,
) -> HostClient<
    impl FnMut(Utf16Range) -> Option<String> + '_,
    impl FnMut() -> Option<Utf16Range> + '_,
    impl FnMut(&str, Utf16Range, Utf16Range) + '_,
    impl FnMut() -> Option<Utf16Range> + '_,
    impl FnMut(&str, Utf16Range) + '_,
    impl FnMut() + '_,
    impl FnMut(Utf16Range) + '_,
> {
    let read_doc = Rc::clone(document);
    let selected_doc = Rc::clone(document);
    let marked_doc = Rc::clone(document);
    let mark_doc = Rc::clone(document);
    let insert_doc = Rc::clone(document);
    let delete_doc = Rc::clone(document);
    let selection_doc = Rc::clone(document);
    HostClient {
        read_text: move |range| read_doc.borrow().read(range),
        selected_range: move || {
            let document = selected_doc.borrow();
            Some(Utf16Range::new(document.caret, 0))
        },
        set_marked_text: Some(move |text: &str, _selection, range: Utf16Range| {
            let mut document = mark_doc.borrow_mut();
            if range.location == usize::MAX {
                document.marked =
                    Some(Utf16Range::new(document.caret, text.encode_utf16().count()));
                document.text.push_str(text);
                document.caret = document.len();
                return;
            }
            document.replace_range(range, text);
            document.marked = Some(Utf16Range::new(range.location, text.encode_utf16().count()));
            document.caret = range.location + text.encode_utf16().count();
        }),
        marked_range: Some(move || marked_doc.borrow().marked),
        insert_text: move |text: &str, range: Utf16Range| {
            let mut document = insert_doc.borrow_mut();
            if range.location == usize::MAX {
                if let Some(marked) = document.marked.take() {
                    document.replace_range(marked, text);
                    document.caret = marked.location + text.encode_utf16().count();
                } else {
                    let start =
                        utf16_byte(&document.text, document.caret).unwrap_or(document.text.len());
                    document.text.insert_str(start, text);
                    document.caret += text.encode_utf16().count();
                }
                return;
            }
            if range.length == usize::MAX {
                return;
            }
            document.marked = None;
            document.replace_range(range, text);
            document.caret = range.location + text.encode_utf16().count();
        },
        delete_backward: Some(move || {
            let mut document = delete_doc.borrow_mut();
            if document.caret == 0 {
                return;
            }
            let start = utf16_byte(&document.text, document.caret - 1).unwrap_or(0);
            let end = utf16_byte(&document.text, document.caret).unwrap_or(document.text.len());
            document.text.replace_range(start..end, "");
            document.caret -= 1;
        }),
        set_selection: Some(move |range: Utf16Range| {
            let mut document = selection_doc.borrow_mut();
            let length = document.len();
            document.caret = range.location.min(length);
        }),
    }
}

fn utf16_byte(text: &str, units: usize) -> Option<usize> {
    if units == 0 {
        return Some(0);
    }
    let mut seen = 0;
    for (index, character) in text.char_indices() {
        if seen == units {
            return Some(index);
        }
        seen += character.len_utf16();
    }
    (seen == units).then_some(text.len())
}

fn read_document(document: &str, range: Utf16Range) -> Option<String> {
    let start = utf16_byte(document, range.location)?;
    let end = utf16_byte(document, range.location + range.length)?;
    Some(document[start..end].to_owned())
}

#[test]
fn range_behind_caret_uses_original_length() {
    let original = "I want to mai coffee.";
    let range = range_behind_caret(
        original.encode_utf16().count(),
        Some(Utf16Range::new(original.encode_utf16().count(), 0)),
    );
    assert_eq!(
        range,
        Some(Utf16Range::new(0, original.encode_utf16().count()))
    );
}

#[test]
fn moved_cursor_still_finds_original() {
    let document = "hello I want to mai coffee. tail";
    let original = "I want to mai coffee.";
    let caret = "hello ".encode_utf16().count() + original.encode_utf16().count();
    let range =
        range_searching_backwards(original, Some(Utf16Range::new(caret, 0)), 1024, |range| {
            read_document(document, range)
        });
    assert_eq!(
        range.map(|range| range.location),
        Some("hello ".encode_utf16().count())
    );
}

#[test]
fn current_line_does_not_cross_newline() {
    let document = "previous line\nI want coffee.";
    let caret = document.encode_utf16().count();
    let (text, _) =
        sentence_on_current_line(Some(document), Some(Utf16Range::new(caret, 0))).unwrap();
    assert_eq!(text, "I want coffee.");
}

#[test]
fn recovers_context_typed_before_tracking() {
    let document = "I want to mai coffee.";
    let tracked = "coffee.";
    let caret = document.encode_utf16().count();
    let (text, range) =
        sentence_before_cursor(Some(document), Some(Utf16Range::new(caret, 0)), tracked).unwrap();
    assert_eq!(text, document);
    assert_eq!(range.location, 0);
}

#[test]
fn edited_original_rejects_without_writing() {
    let document = Rc::new(RefCell::new(Document::new("I want tea.")));
    let original = "I want coffee.";
    let range = Utf16Range::new(0, original.encode_utf16().count());
    let outcome = {
        let mut client = client(&document);
        CommittedTextReplacement::replace_range(original, "I want coffee.", range, &mut client)
    };
    assert_eq!(outcome, ReplacementOutcome::Stale);
    assert_eq!(document.borrow().text, "I want tea.");
}

#[test]
fn honest_host_replaces_once() {
    let original = "I want to mai coffee.";
    let document = Rc::new(RefCell::new(Document::new(original)));
    let range = Utf16Range::new(0, original.encode_utf16().count());
    let outcome = {
        let mut client = client(&document);
        CommittedTextReplacement::replace_range(
            original,
            "I want to buy coffee.",
            range,
            &mut client,
        )
    };
    assert_eq!(outcome, ReplacementOutcome::Replaced);
    assert_eq!(document.borrow().text, "I want to buy coffee.");
}

#[test]
fn preferred_range_keeps_exact_match() {
    let original = "mai.";
    let document = "xxmai.";
    let caret = document.encode_utf16().count();
    let recovered = range_of_original(
        Some(document),
        0,
        Some(Utf16Range::new(caret - 1, 0)),
        original,
    );
    let behind = range_behind_caret(
        original.encode_utf16().count(),
        Some(Utf16Range::new(caret, 0)),
    );
    let chosen = preferred_range(original, &[behind, recovered], |range| {
        read_document(document, range)
    });
    assert_eq!(chosen, recovered);
}

#[test]
fn accepts_auto_capitalised_sentence() {
    assert!(CommittedTextReplacement::matches_loosely(
        Some("Nihao?"),
        "nihao?"
    ));
    assert!(!CommittedTextReplacement::matches_loosely(
        Some("hello"),
        "hi"
    ));
}
