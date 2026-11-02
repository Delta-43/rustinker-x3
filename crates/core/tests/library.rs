use x3_core::{library::*, storage::Record};

fn rec(item: &str, generation: u32) -> Record {
    Record::new(item, generation, 100, [0; 32], false).unwrap()
}

fn names(s: &Shelf) -> std::vec::Vec<std::string::String> {
    s.articles().map(|r| r.item().into()).collect()
}

#[test]
fn latest_generation_wins_in_any_order() {
    let mut s = Shelf::default();
    s.add(rec("b", 5));
    s.add(rec("a", 2));
    let mut newer = rec("a", 2);
    newer.generation = 9; // a position save: same payload, newer metadata
    newer.position = 40;
    s.add(newer);
    s.add(rec("a", 1)); // older, ignored
    let a = s.articles().find(|r| r.item() == "a").unwrap();
    assert_eq!((a.generation, a.position), (9, 40));
    // Newest payload first: b (5) before a (2).
    assert_eq!(names(&s), ["b", "a"]);
}

#[test]
fn tombstones_hide_and_tokens_are_ignored() {
    let mut s = Shelf::default();
    s.add(rec("gone", 3));
    let mut dead = rec("gone", 3);
    dead.generation = 4;
    dead.deleted = true;
    s.add(dead);
    s.add(Record::new("tok", 7, 100, [0; 32], true).unwrap());
    assert!(s.is_empty());
    // A tombstone older than the live record does not hide it.
    let mut s = Shelf::default();
    let mut old_dead = rec("x", 1);
    old_dead.deleted = true;
    s.add(old_dead);
    s.add(rec("x", 2));
    assert_eq!(names(&s), ["x"]);
}

#[test]
fn bounded_keeps_the_newest() {
    let mut s = Shelf::default();
    for i in 1..=(MAX_ITEMS as u32 + 10) {
        s.add(rec(&std::format!("item{i}"), i));
    }
    assert_eq!(s.len(), MAX_ITEMS);
    assert_eq!(names(&s)[0], std::format!("item{}", MAX_ITEMS + 10));
    assert!(!names(&s).contains(&"item1".into()));
}

#[test]
fn truncate_respects_characters() {
    assert_eq!(truncate("żółw", 3), "ż"); // ż is 2 bytes, ó would split
    assert_eq!(truncate("abc", 10), "abc");
    assert_eq!(truncate("", 0), "");
}

#[test]
fn token_names_come_from_the_item_id() {
    use x3_core::library::token_name;
    let mut b = [0u8; 48];
    for (item, want) in [
        ("t-goblin-warrior-0123456789ab", "Goblin warrior"),
        ("t-1-1-elf-0123456789ab", "1 1 elf"),
        ("t-treasure-abcdefabcdef", "Treasure"),
        ("t-0123456789abcdef0123456789abcdef", "Token"),
        ("t-name-xyz", "Token"),
        ("t--0123456789ab", "Token"),
        ("t-Bad-0123456789ab", "Token"),
        ("a-article-0123456789ab", "Picture"),
        ("t-", "Token"),
        ("", "Picture"),
        ("i-harbour-at-dusk-0123456789ab", "Harbour at dusk"),
        ("i-0123456789abcdef0123456789abcdef", "Picture"),
        ("i-Bad-0123456789ab", "Picture"),
        ("i-", "Picture"),
    ] {
        assert_eq!(token_name(item, &mut b), want, "{item}");
    }
}
