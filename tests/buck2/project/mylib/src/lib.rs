//! A small library crate with non-generic and generic functions, used through `--extern`.
use std::collections::HashMap;

/// Non-generic: its body is only in the rmeta when built with `-Zalways-encode-mir`.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Generic: its MIR is always encoded.
pub fn histogram<T: std::hash::Hash + Eq + Clone>(items: &[T]) -> HashMap<T, usize> {
    let mut map = HashMap::new();
    for item in items {
        *map.entry(item.clone()).or_insert(0) += 1;
    }
    map
}

pub fn checked_double(x: u32) -> Option<u32> {
    x.checked_mul(2)
}
