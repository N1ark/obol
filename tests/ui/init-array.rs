// Statics in `.init_array` hold constructors run before `main` (as registered by `inventory`):
// they are translated even though nothing refers to them, and keep their link section.
use std::sync::atomic::{AtomicBool, Ordering};

static REGISTERED: AtomicBool = AtomicBool::new(false);

extern "C" fn register() {
    REGISTERED.store(true, Ordering::Relaxed);
}

#[used]
#[unsafe(link_section = ".init_array")]
static INIT: extern "C" fn() = register;

fn main() {
    assert!(REGISTERED.load(Ordering::Relaxed));
}
