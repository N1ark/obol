//! A pointer constant whose pointee type doesn't describe its allocation points to raw memory of
//! the allocation's size and alignment. When the allocation holds padding or pointers, as here
//! (like the `SimpleMessage` behind `io::Error::READ_EXACT_EOF`), it is kept as raw memory.
struct Message {
    message: &'static str,
    kind: u8,
}

const MESSAGE: *const () = &Message {
    message: "failed to fill whole buffer",
    kind: 37,
} as *const Message as *const ();

fn message() -> *const () {
    MESSAGE
}
