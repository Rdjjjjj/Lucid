use lucid_core::{SentenceCompletion, SentenceTracker};

#[test]
fn completes_punctuation_and_tracks_utf16_range() {
    let mut tracker = SentenceTracker::new();
    let completed = tracker.append("I want to mai coffee.");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].text, "I want to mai coffee.");
    assert_eq!(completed[0].utf16_range.location, 0);
    assert_eq!(
        completed[0].utf16_range.length,
        "I want to mai coffee.".encode_utf16().count()
    );
}

#[test]
fn flushes_unpunctuated_sentence_after_pause() {
    let mut tracker = SentenceTracker::new();
    assert!(tracker.append("this coffee tai haohe").is_empty());
    let sentence = tracker.flush_on_pause().expect("pause finishes the draft");
    assert_eq!(sentence.text, "this coffee tai haohe");
}

#[test]
fn tracks_multiple_sentences_and_offsets() {
    let mut tracker = SentenceTracker::new();
    let first = tracker.append("Hi.");
    let second = tracker.append(" Bye!");
    assert_eq!(first[0].utf16_range.location, 0);
    assert_eq!(second[0].text, "Bye!");
    assert_eq!(
        second[0].utf16_range.location,
        "Hi. ".encode_utf16().count()
    );
}

#[test]
fn streamed_decimal_point_does_not_end_sentence() {
    let mut tracker = SentenceTracker::new();
    assert!(tracker.append("price is 3.").is_empty());
    assert!(tracker.append("14 now").is_empty());
    assert_eq!(tracker.pending_text(), "price is 3.14 now");
}

#[test]
fn delete_backward_removes_last_character() {
    let mut tracker = SentenceTracker::new();
    tracker.append("mai");
    tracker.delete_backward();
    assert_eq!(tracker.pending_text(), "ma");
}

#[test]
fn reset_clears_session() {
    let mut tracker = SentenceTracker::new();
    tracker.append("hello");
    let before = tracker.version();
    tracker.reset();
    assert!(tracker.pending_text().is_empty());
    assert_ne!(tracker.version(), before);
}

#[test]
fn looks_finished_ignores_incomplete_phrases() {
    assert!(!SentenceCompletion::looks_finished("I want to"));
    assert!(!SentenceCompletion::looks_finished("hello,"));
    assert!(SentenceCompletion::looks_finished("I want coffee now"));
    assert!(SentenceCompletion::looks_finished("I want to mai coffee."));
}
