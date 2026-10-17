//! Unit tests for `crate::universes`.

use super::date;
use crate::{StaticUniverse, Universe};

#[test]
fn static_universe_answers_membership_and_position() {
    let u = StaticUniverse::new(date(2025, 1, 1), vec!["A".into(), "B".into()]);
    assert_eq!(u.as_of(), date(2025, 1, 1));
    assert_eq!(u.len(), 2);
    assert!(!u.is_empty());
    assert!(u.contains("B"));
    assert!(!u.contains("b"));
    assert_eq!(u.position("B"), Some(1));
    assert_eq!(u.position("Z"), None);
}

#[test]
fn empty_universe_is_empty() {
    let u = StaticUniverse::new(date(2025, 1, 1), vec![]);
    assert!(u.is_empty());
    assert_eq!(u.len(), 0);
}
