//! A reference to a constant with a slice tail (and padding) keeps its length metadata.
struct W<T: ?Sized> {
    tag: u8,
    data: T,
}

const A: &W<[u16]> = &W {
    tag: 1,
    data: [1, 2, 3],
};

fn main() {
    let a = A;
    let _ = (a.tag, a.data.len());
}
