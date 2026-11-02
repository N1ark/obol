//@ rustc-args=--crate-type=bin --target=x86_64-unknown-linux-gnu
//@ obol-args=--start-from main
//! Only the `#[used]` statics in the target's constructor section are entrypoints.

extern "C" fn elf_ctor() {}
extern "C" fn elf_ctor_a() {}
extern "C" fn elf_ctor_b() {}
extern "C" fn macho_ctor() {}
extern "C" fn coff_ctor() {}
extern "C" fn not_used() {}
extern "C" fn other_section() {}

#[used]
#[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".init_array"))]
static ELF_CTOR: extern "C" fn() = elf_ctor;

#[used]
#[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".init_array"))]
static ELF_CTORS: [extern "C" fn(); 2] = [elf_ctor_a, elf_ctor_b];

#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static MACHO_CTOR: extern "C" fn() = macho_ctor;

#[used]
#[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".CRT$XCU"))]
static COFF_CTOR: extern "C" fn() = coff_ctor;

#[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".init_array"))]
static NOT_USED: extern "C" fn() = not_used;

#[used]
#[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".data"))]
static OTHER_SECTION: extern "C" fn() = other_section;

fn main() {}
