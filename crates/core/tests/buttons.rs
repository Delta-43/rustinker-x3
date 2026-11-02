use x3_core::buttons::*;

#[test]
fn test_unit_readings_decode() {
    // Left to right along the bottom: Left, Right, Back, Confirm (ADR 024).
    assert_eq!(bottom(3518), Some(Button::Left));
    assert_eq!(bottom(2685), Some(Button::Right));
    assert_eq!(bottom(1486), Some(Button::Back));
    assert_eq!(bottom(4), Some(Button::Confirm));
    assert_eq!(side(2236), Some(Button::Up));
    assert_eq!(side(3), Some(Button::Down));
    assert_eq!(bottom(4095), None);
    assert_eq!(side(4095), None);
}

#[test]
fn band_edges_and_extremes() {
    assert_eq!(bottom(3900), Some(Button::Left));
    assert_eq!(bottom(3901), None);
    assert_eq!(bottom(3101), Some(Button::Left));
    assert_eq!(bottom(3100), Some(Button::Right));
    assert_eq!(bottom(2090), Some(Button::Back));
    assert_eq!(bottom(750), Some(Button::Confirm));
    assert_eq!(bottom(0), Some(Button::Confirm));
    assert_eq!(bottom(u16::MAX), None);
    assert_eq!(side(1121), Some(Button::Up));
    assert_eq!(side(1120), Some(Button::Down));
    assert_eq!(side(u16::MAX), None);
}

#[test]
fn decode_priority_and_adc_failure() {
    assert_eq!(decode(Some(4), Some(3), true), Some(Button::Power));
    assert_eq!(decode(Some(4), Some(3), false), Some(Button::Confirm));
    assert_eq!(decode(Some(4095), Some(3), false), Some(Button::Down));
    assert_eq!(decode(None, None, false), None);
    assert_eq!(decode(None, Some(2236), false), Some(Button::Up));
}

fn run(samples: &[Option<Button>]) -> std::vec::Vec<Button> {
    let mut d = Debouncer::default();
    samples.iter().filter_map(|&s| d.feed(s)).collect()
}

#[test]
fn one_event_per_press() {
    use Button::*;
    let press = [
        None,
        Some(Back),
        Some(Back),
        Some(Back),
        Some(Back),
        None,
        None,
    ];
    assert_eq!(run(&press), [Back]);
    // Held for a long time: still one event.
    let mut long = std::vec![Some(Confirm); 500];
    long.push(None);
    assert_eq!(run(&long), [Confirm]);
}

#[test]
fn bounce_and_ladder_ramps_are_ignored() {
    use Button::*;
    // A one-sample glitch never counts.
    assert_eq!(run(&[None, Some(Left), None, Some(Up), None]), []);
    // Release ramping through Confirm/Left on the way to idle.
    let s = [
        Some(Back),
        Some(Back),
        Some(Confirm),
        Some(Left),
        None,
        None,
    ];
    assert_eq!(run(&s), [Back]);
    // Bounce on release, then a real second press.
    let s = [
        Some(Down),
        Some(Down),
        None,
        Some(Down),
        None,
        None,
        Some(Down),
        Some(Down),
    ];
    assert_eq!(run(&s), [Down, Down]);
}

#[test]
fn switching_buttons_without_release() {
    use Button::*;
    let s = [Some(Up), Some(Up), Some(Down), Some(Down), Some(Down)];
    assert_eq!(run(&s), [Up, Down]);
}

#[test]
fn a_primed_debouncer_ignores_the_key_already_down_and_takes_the_next() {
    use x3_core::buttons::{Button, Debouncer};
    let mut d = Debouncer::primed(Some(Button::Confirm));
    // Confirm still held from the press that started the refresh.
    for _ in 0..5 {
        assert_eq!(d.feed(Some(Button::Confirm)), None);
    }
    // Released, then a new press during the refresh counts once.
    assert_eq!(d.feed(None), None);
    assert_eq!(d.feed(Some(Button::Down)), None);
    assert_eq!(d.feed(Some(Button::Down)), Some(Button::Down));
    assert_eq!(d.feed(Some(Button::Down)), None);
    // Nothing down at the start: the first press counts.
    let mut d = Debouncer::primed(None);
    assert_eq!(d.feed(Some(Button::Back)), None);
    assert_eq!(d.feed(Some(Button::Back)), Some(Button::Back));
}
