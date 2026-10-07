//! Understanding Buck2's rustc invocations.
//!
//! The Buck2 prelude never runs rustc directly: every Rust compile action is
//!
//! ```text
//! python3 rustc_action.py @<crate>-<emit>.args
//! ```
//!
//! where the argsfile contains wrapper options (`--env=K=V`, `--path-env=K=P`, `--diag-json=…`,
//! `--required-output SHORT PATH`, …), then the compiler command, then `--rustc-action-separator`,
//! then the actual rustc arguments (which reference more argsfiles, e.g. the `-Ldependency` one).
//! See `prelude/rust/tools/rustc_action.py`.
//!
//! This module parses such a command line and turns it into an `obol-driver` invocation that
//! reproduces what `rustc_action.py` would have run (environment, argument rewriting), minus the
//! output-producing flags.

use std::path::Path;

pub const SEPARATOR: &str = "--rustc-action-separator";

/// Placeholder for a build script's `OUT_DIR` in rustc arguments (see `rustc_action.py`).
const OUT_DIR_SENTINEL: &str = "${__BUILDSCRIPT_OUT_DIR__}";

/// A parsed `rustc_action.py` command line.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RustcAction {
    /// `--env=K=V` (values already unescaped like `rustc_action.py` does).
    pub env: Vec<(String, String)>,
    /// `--path-env=K=P`: `P` is relative to the project root and must be absolutized.
    pub path_env: Vec<(String, String)>,
    pub remap_cwd_prefix: Option<String>,
    pub buck_target: Option<String>,
    /// The compiler command (usually a single path).
    pub compiler: Vec<String>,
    /// The rustc arguments, unmodified (may contain `@argsfiles`).
    pub rustc_args: Vec<String>,
}

/// The pieces needed to run the driver in place of the compiler.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DriverInvocation {
    /// Environment variables to set (on top of the inherited ones).
    pub env: Vec<(String, String)>,
    /// Arguments for the driver (without the program name).
    pub args: Vec<String>,
}

fn split_kv(s: &str) -> Result<(String, String), String> {
    s.split_once('=')
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .ok_or_else(|| format!("expected `key=value`, found `{s}`"))
}

/// Unescape newlines like `rustc_action.py`: `\\n` -> newline, `\\\n` -> `\\n`.
fn unescape_env(v: &str) -> String {
    v.replace("\\n", "\n").replace("\\\n", "\\n")
}

impl RustcAction {
    /// Parse the (top-level-expanded) arguments of `rustc_action.py`.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let sep = args
            .iter()
            .position(|a| a == SEPARATOR)
            .ok_or_else(|| format!("not a rustc_action command line: no `{SEPARATOR}` found"))?;
        let (wrapper, rest) = args.split_at(sep);
        let rustc_args = rest[1..].to_vec();
        let mut this = RustcAction {
            rustc_args,
            ..Default::default()
        };
        // Options taking a value either as `--opt=value` or `--opt value`.
        const VALUE_OPTS: &[&str] = &[
            "--diag-json",
            "--diag-txt",
            "--env",
            "--path-env",
            "--remap-cwd-prefix",
            "--crate-map",
            "--buck-target",
            "--failure-filter",
            "--echo",
        ];
        let mut i = 0;
        while i < wrapper.len() {
            let arg = &wrapper[i];
            if arg == "--required-output" {
                // nargs=2
                i += 3;
                continue;
            }
            let (name, value) = if let Some((n, v)) = arg.split_once('=')
                && VALUE_OPTS.contains(&n)
            {
                (n, v.to_owned())
            } else if VALUE_OPTS.contains(&arg.as_str()) {
                i += 1;
                let v = wrapper
                    .get(i)
                    .ok_or_else(|| format!("missing value for `{arg}`"))?;
                (arg.as_str(), v.clone())
            } else if arg.starts_with("--") {
                return Err(format!("unknown rustc_action.py option `{arg}`"));
            } else {
                // The compiler command: everything up to the separator.
                this.compiler = wrapper[i..].to_vec();
                break;
            };
            match name {
                "--env" => {
                    let (k, v) = split_kv(&value)?;
                    this.env.push((k, unescape_env(&v)));
                }
                "--path-env" => this.path_env.push(split_kv(&value)?),
                "--remap-cwd-prefix" => this.remap_cwd_prefix = Some(value),
                "--buck-target" => this.buck_target = Some(value),
                _ => {}
            }
            i += 1;
        }
        if this.compiler.is_empty() {
            return Err("no compiler found before `--rustc-action-separator`".to_owned());
        }
        Ok(this)
    }

    /// Build the driver invocation: environment and rewritten rustc arguments.
    ///
    /// `project_root` is the directory the action runs in (Buck runs actions from the project
    /// root, and all paths in the command line are relative to it). `out_dir` replaces the
    /// action's `--out-dir` (Obol does not emit artifacts, but rustc wants a valid directory).
    pub fn driver_invocation(
        &self,
        project_root: &Path,
        out_dir: &Path,
    ) -> Result<DriverInvocation, String> {
        let mut env = self.env.clone();
        for (k, p) in &self.path_env {
            env.push((k.clone(), absolutize(project_root, p)));
        }
        let out_dir_env = env
            .iter()
            .rev()
            .find(|(k, _)| k == "OUT_DIR")
            .map(|(_, v)| v.clone());

        let mut args = Vec::new();
        let mut iter = self.rustc_args.iter();
        while let Some(arg) = iter.next() {
            let mut arg = arg_eval(arg, project_root)?;
            if let Some(out) = &out_dir_env {
                arg = arg.replace(OUT_DIR_SENTINEL, out);
            }
            if let Some(rest) = arg.strip_prefix("--remap-path-prefix=") {
                let mut parts = rest.splitn(2, '=');
                let from = parts.next().unwrap_or_default();
                let mut to = parts.next().unwrap_or_default();
                if let Some(t) = to.strip_prefix("./") {
                    to = t;
                }
                arg = format!("--remap-path-prefix={from}={to}");
            }
            if let Some(kv) = arg.strip_prefix("--env-set=") {
                env.push(split_kv(kv)?);
                continue;
            }
            // Output-related flags: Obol emits no artifacts.
            if arg.starts_with("--emit=") || arg.starts_with("--out-dir=") {
                continue;
            }
            if arg == "--emit" || arg == "--out-dir" || arg == "-o" {
                iter.next();
                continue;
            }
            if arg.starts_with("-Cincremental=") {
                continue;
            }
            // Keep diagnostics human-readable: `rustc_action.py` renders the JSON for us, we don't.
            if arg.starts_with("--error-format=") || arg.starts_with("--json=") {
                continue;
            }
            args.push(arg);
        }
        if let Some(prefix) = &self.remap_cwd_prefix {
            let root = project_root.display().to_string();
            args.push(format!("--remap-path-prefix={root}={prefix}"));
            if let Ok(real) = project_root.canonicalize() {
                args.push(format!("--remap-path-prefix={}={prefix}", real.display()));
            }
        }
        args.push(format!("--out-dir={}", out_dir.display()));
        Ok(DriverInvocation { env, args })
    }
}

fn absolutize(root: &Path, p: &str) -> String {
    let path = Path::new(p);
    if path.is_absolute() {
        p.to_owned()
    } else {
        root.join(path).display().to_string()
    }
}

/// Expand `$(cat path)` and `$(abspath path)` like `rustc_action.py`'s `arg_eval`.
pub fn arg_eval(arg: &str, project_root: &Path) -> Result<String, String> {
    let mut expanded = String::new();
    let mut rest = arg;
    while let Some(begin) = rest.find("$(") {
        expanded.push_str(&rest[..begin]);
        let tail = &rest[begin..];
        if let Some(t) = tail.strip_prefix("$(cat ") {
            let (path, r) = t
                .split_once(')')
                .ok_or_else(|| format!("unterminated `$(cat` in `{arg}`"))?;
            let contents = std::fs::read_to_string(project_root.join(path))
                .map_err(|e| format!("reading `{path}` for `{arg}`: {e}"))?;
            expanded.push_str(contents.trim());
            rest = r;
        } else if let Some(t) = tail.strip_prefix("$(abspath ") {
            let (path, r) = t
                .split_once(')')
                .ok_or_else(|| format!("unterminated `$(abspath` in `{arg}`"))?;
            expanded.push_str(&absolutize(project_root, path));
            rest = r;
        } else {
            expanded.push_str("$(");
            rest = &tail[2..];
        }
    }
    expanded.push_str(rest);
    Ok(expanded)
}

/// All `buck-out/...` paths mentioned in some arguments (in `--extern=name=buck-out/…`,
/// `-Ldependency=buck-out/…`, `@buck-out/…`, `--remap-path-prefix=buck-out/…=…`, plain paths, …).
/// Used to ask Buck to materialize the inputs of an action.
pub fn buck_out_paths(args: &[String], buck_out: &str) -> Vec<String> {
    let needle = format!("{buck_out}/");
    let mut out = Vec::new();
    for arg in args {
        let mut s = arg.as_str();
        while let Some(pos) = s.find(&needle) {
            // Only take matches at the start of a "word".
            let ok = pos == 0 || matches!(s.as_bytes()[pos - 1], b'=' | b'@' | b':' | b',');
            let tail = &s[pos..];
            let end = tail.find('=').unwrap_or(tail.len());
            if ok {
                out.push(tail[..end].trim_end_matches('/').to_owned());
            }
            s = &tail[end..];
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn sample() -> Vec<String> {
        s(&[
            "--diag-json=buck-out/v2/art/app/out/XIPMD/app-diag.json",
            "--buck-target=root//app:app",
            "--crate-map=mylib=root//mylib:mylib",
            "--env=RUSTC_BOOTSTRAP=1",
            "--env=MSG=a\\nb",
            "--path-env=OUT_DIR=buck-out/v2/gen/out_dir",
            "--required-output",
            "XIPMD/libapp.rmeta",
            "buck-out/v2/art/app/out/XIPMD/libapp.rmeta",
            "/opt/rustc",
            SEPARATOR,
            "buck-out/v2/art/app/abc/__srcs/src/main.rs",
            "--crate-name=app",
            "--error-format=json",
            "--json=diagnostic-rendered-ansi",
            "--extern=noprelude,nounused:core=buck-out/v2/art/core/123/LPPM/libcore-1.rmeta",
            "@buck-out/v2/art/app/XIPMD-depsfast-symlinked_dirs/dirs",
            "--remap-path-prefix=buck-out/v2/art/app/abc/__srcs/=./app/",
            "--env-set=FOO=bar",
            "--cfg=x=\"${__BUILDSCRIPT_OUT_DIR__}/y\"",
            "--emit=metadata=buck-out/v2/art/app/out/XIPMD/libapp.rmeta",
            "--out-dir=buck-out/v2/art/app/XIPMD/extras/libapp",
        ])
    }

    #[test]
    fn parse_action() {
        let a = RustcAction::parse(&sample()).unwrap();
        assert_eq!(a.compiler, s(&["/opt/rustc"]));
        assert_eq!(a.buck_target.as_deref(), Some("root//app:app"));
        assert_eq!(
            a.env,
            vec![
                ("RUSTC_BOOTSTRAP".into(), "1".into()),
                ("MSG".into(), "a\nb".into())
            ]
        );
        assert_eq!(a.rustc_args.len(), 11);
        assert!(RustcAction::parse(&s(&["rustc", "--edition=2021"])).is_err());
    }

    #[test]
    fn invocation() {
        let a = RustcAction::parse(&sample()).unwrap();
        let inv = a
            .driver_invocation(Path::new("/proj"), Path::new("/tmp/out"))
            .unwrap();
        assert!(
            inv.env
                .contains(&("OUT_DIR".into(), "/proj/buck-out/v2/gen/out_dir".into()))
        );
        assert!(inv.env.contains(&("FOO".into(), "bar".into())));
        assert_eq!(
            inv.args,
            s(&[
                "buck-out/v2/art/app/abc/__srcs/src/main.rs",
                "--crate-name=app",
                "--extern=noprelude,nounused:core=buck-out/v2/art/core/123/LPPM/libcore-1.rmeta",
                "@buck-out/v2/art/app/XIPMD-depsfast-symlinked_dirs/dirs",
                "--remap-path-prefix=buck-out/v2/art/app/abc/__srcs/=app/",
                "--cfg=x=\"/proj/buck-out/v2/gen/out_dir/y\"",
                "--out-dir=/tmp/out",
            ])
        );
    }

    #[test]
    fn eval() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("name.txt"), "foo\n").unwrap();
        assert_eq!(
            arg_eval("--extern=$(cat name.txt)=x.rlib", dir.path()).unwrap(),
            "--extern=foo=x.rlib"
        );
        assert_eq!(
            arg_eval("--env-set=X=$(abspath a/b)", Path::new("/r")).unwrap(),
            "--env-set=X=/r/a/b"
        );
        assert_eq!(arg_eval("$(other)", Path::new("/r")).unwrap(), "$(other)");
    }

    #[test]
    fn paths() {
        let paths = buck_out_paths(&sample(), "buck-out/v2");
        assert!(paths.contains(&"buck-out/v2/art/core/123/LPPM/libcore-1.rmeta".to_owned()));
        assert!(
            paths.contains(&"buck-out/v2/art/app/XIPMD-depsfast-symlinked_dirs/dirs".to_owned())
        );
        assert!(paths.contains(&"buck-out/v2/art/app/abc/__srcs".to_owned()));
        assert!(paths.contains(&"buck-out/v2/art/app/abc/__srcs/src/main.rs".to_owned()));
    }
}
