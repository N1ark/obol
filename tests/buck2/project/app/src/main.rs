fn main() {
    let text = String::from("the quick brown fox jumps over the lazy dog");
    let n = mylib::word_count(&text);
    let words: Vec<&str> = text.split(' ').collect();
    let h = mylib::histogram(&words);
    assert_eq!(n, 9);
    assert_eq!(h["the"], 2);
    assert_eq!(mylib::checked_double(21), Some(42));
    println!("{n}");
}
