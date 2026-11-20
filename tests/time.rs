//! Tests for src/time.rs: the clock, and times as people write and read them.

use nxf::time;

/// The clock is the system's, in whole seconds.
#[test]
fn now() {
    let system = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(time::now().abs_diff(system) <= 1);
}

/// A time is what `date --date` reads; anything else is refused with a hint.
#[test]
fn parse() {
    assert_eq!(time::parse("@1790000000"), Ok(1_790_000_000));
    assert_eq!(
        time::parse("2030-01-31 18:00"),
        time::parse("2030-01-31 18:00")
    );
    assert!(time::parse("2030-01-31").unwrap() < time::parse("2030-02-01").unwrap());
    let error = time::parse("next never").unwrap_err();
    assert_eq!(
        error,
        "Invalid time 'next never'; use a date such as 2030-01-31 or '2030-01-31 18:00'."
    );
}

/// A time for people is the date and the minute, in the local time zone.
#[test]
fn shown() {
    let text = time::shown(1_790_000_000);
    let digits: Vec<bool> = text.chars().map(|ch| ch.is_ascii_digit()).collect();
    assert_eq!(text.len(), "2026-09-21 14:13".len(), "{text}");
    assert_eq!(&text[4..5], "-");
    assert_eq!(&text[10..11], " ");
    assert!(digits[..4].iter().all(|digit| *digit));
}

/// How long ago is counted in the largest unit that fits, rounded down; the
/// times are far from the edges of the units, so that the clock may tick.
#[test]
fn age() {
    let ago = |seconds: u64| time::age(time::now() - seconds);
    assert_eq!(ago(0), "just now");
    assert_eq!(ago(30), "just now");
    assert_eq!(ago(90), "1 minute ago");
    assert_eq!(ago(7_000), "1 hour ago");
    assert_eq!(ago(7_300), "2 hours ago");
    assert_eq!(ago(86_400 * 3 + 5), "3 days ago");
    // A time ahead of the clock is not ago.
    assert_eq!(time::age(time::now() + 100), "just now");
}
