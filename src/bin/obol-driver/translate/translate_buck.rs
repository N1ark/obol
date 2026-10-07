//! Support for crate graphs not built by cargo + rustup (Buck2 in particular): std crates are
//! recognized by name rather than by sysroot path, and missing dependency MIR is reported.
extern crate rustc_hir;
extern crate rustc_public;
extern crate rustc_span;

use charon_lib::ast::FileName;
use rustc_public::{CrateDef, mir, rustc_internal};

use crate::translate::translate_ctx::{ItemTransCtx, TranslateCtx};

impl<'tcx> TranslateCtx<'tcx> {
    /// Adjust a translated file name for crate graphs built outside of cargo/rustup:
    /// * files of standard-library crates are normalized to `/rustc/library/...`, the form Obol
    ///   produces for a rustup sysroot, whatever the layout (under Buck, std is built from e.g.
    ///   `third-party/rust-toolchain/<ver>/rust/library/` and its paths are remapped by Buck);
    /// * Buck compiles sources through a symlink tree in `buck-out` and passes
    ///   `--remap-path-prefix=<buck-out srcs dir>=<package>/`: we use the remapped,
    ///   project-relative name rather than the `buck-out` one.
    pub(crate) fn normalize_buck_filename(
        &self,
        name: FileName,
        rustc_name: &rustc_span::FileName,
        cnum: rustc_span::def_id::CrateNum,
    ) -> FileName {
        let name = match (&name, rustc_name) {
            (FileName::Local(p), rustc_span::FileName::Real(real)) if is_in_buck_out(p) => {
                let remapped = real.path(rustc_span::RemapPathScopeComponents::MACRO);
                if remapped != p.as_path() {
                    FileName::Local(remapped.to_path_buf())
                } else {
                    name
                }
            }
            _ => name,
        };
        let crate_name = self.tcx.crate_name(cnum);
        let crate_name = crate_name.as_str();
        match &name {
            // Like files of a rustup sysroot, which `translate_filename` makes `Local`.
            FileName::Local(p) | FileName::Virtual(p)
                if let Some(std) = obol_lib::std_crates::normalize_std_path(crate_name, p) =>
            {
                FileName::Local(std)
            }
            // Dependencies built with `--remap-path-prefix=<srcs>=<package>/` (Buck) have
            // project-relative names; when they resolve from the working directory (the project
            // root under `obol buck2`), they are real files.
            FileName::Virtual(p) if p.is_relative() && p.is_file() => FileName::Local(p.clone()),
            _ => name,
        }
    }
}

fn is_in_buck_out(p: &std::path::Path) -> bool {
    p.components()
        .any(|c| matches!(c, std::path::Component::Normal(n) if n == "buck-out"))
}

impl ItemTransCtx<'_, '_> {
    /// Record that a non-local function has no MIR (see [`crate::missing_mir`]).
    pub(crate) fn record_missing_mir(&self, def: mir::mono::Instance) {
        let tcx = self.t_ctx.tcx;
        let def_id = rustc_internal::internal(tcx, def.def.def_id());
        if def_id.is_local() {
            return;
        }
        // Items for which no MIR is expected even in a full-MIR build.
        if tcx.is_foreign_item(def_id) || tcx.intrinsic(def_id).is_some() {
            return;
        }
        crate::missing_mir::record(tcx.crate_name(def_id.krate).as_str(), def.name());
    }
}
