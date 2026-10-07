pub fn average_word_len(text: &str) -> Option<usize> {
    let n = mylib::word_count(text);
    if n == 0 {
        return None;
    }
    let total: usize = text.split_whitespace().map(|w| w.len()).sum();
    Some(total / n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert_eq!(average_word_len(""), None);
    }

    #[test]
    fn some_words() {
        assert_eq!(average_word_len("ab abcd"), Some(3));
    }
}
