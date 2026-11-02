extern crate std;
use std::{string::String, vec::Vec};
use x3_core::text::*;
#[test]
fn polish_and_greedy_golden() {
    let lines: Vec<_> = Lines::new("# Żółć\n\nZażółć gęślą jaźń", 8, 0)
        .unwrap()
        .collect();
    assert!(lines[0].heading);
    assert_eq!(
        lines.iter().map(|l| l.text).collect::<Vec<_>>(),
        ["Żółć", "", "Zażółć", "gęślą", "jaźń"]
    );
}
#[test]
fn long_words_and_soft_hyphens() {
    let lines: Vec<_> = Lines::new("abc\u{ad}defgh", 5, 0).unwrap().collect();
    assert_eq!(lines[0].text, "abc");
    assert!(lines[0].hyphen);
    assert_eq!(lines[1].text, "defgh");
    let lines: Vec<_> = Lines::new("ążźćńółę", 3, 0).unwrap().collect();
    assert_eq!(
        lines
            .iter()
            .map(|x| x.text.chars().count())
            .collect::<Vec<_>>(),
        [3, 3, 2]
    );
    let lines: Vec<_> = Lines::new("a\u{ad}b", 3, 0).unwrap().collect();
    assert!(!lines[0].hyphen);
}
#[test]
fn pages_resume_without_losing_unicode() {
    let text = "ąęćńół\nŻółw 😀";
    let mut at = 0;
    let mut ranges = Vec::new();
    while at < text.len() {
        let next = page(text, 3, 1, at, |_, line| {
            ranges.push((line.start, line.end))
        })
        .unwrap();
        assert!(next > at);
        assert!(text.is_char_boundary(next));
        at = next;
    }
    assert_eq!(ranges.first().unwrap().0, 0);
    assert_eq!(ranges.last().unwrap().1, text.len());
}
#[test]
fn rejects_invalid_limits_and_controls() {
    assert!(Lines::new("", 0, 0).is_err());
    assert!(Lines::new("ą", 1, 1).is_err());
    assert!(Lines::new("\0", 1, 0).is_err());
    assert!(Lines::new("x", 1, 2).is_err());
    assert!(Lines::new(&String::from("a").repeat(MAX_TEXT + 1), 1, 0).is_err());
    assert!(page("x", 1, 0, 0, |_, _| {}).is_err());
    assert_eq!(Lines::new("", 1, 0).unwrap().count(), 0);
}
