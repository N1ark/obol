//! Expansion of rustc-style `@argsfile` arguments.
//!
//! Buck2 passes almost all rustc arguments through argsfiles (one argument per line), and those
//! argsfiles themselves reference further argsfiles (e.g. `@.../deps-symlinked_dirs/dirs`, which
//! holds the `-Ldependency=` flags). rustc expands them itself, but the driver and the `obol buck2`
//! command need to look at the expanded command line (to find `--test`, `--crate-name`, the
//! `--extern` artifacts, ...), so we expand them the same way rustc does.

use std::io;
use std::path::{Path, PathBuf};

/// Expand all `@file` and `@shell:file` arguments, recursively, the way rustc does:
/// * `@file`: each line of the file is one argument (no quoting);
/// * `@shell:file`: the file is split into words like a POSIX shell would (quotes, backslashes).
///
/// Relative paths are resolved against `cwd`.
pub fn expand_argsfiles(args: &[String], cwd: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        expand_one(arg, cwd, &mut out, 0)?;
    }
    Ok(out)
}

fn expand_one(arg: &str, cwd: &Path, out: &mut Vec<String>, depth: usize) -> io::Result<()> {
    if depth > 32 {
        return Err(io::Error::other(format!(
            "argsfile nesting too deep while expanding `{arg}`"
        )));
    }
    let Some(file) = arg.strip_prefix('@') else {
        out.push(arg.to_owned());
        return Ok(());
    };
    let (file, shell) = match file.strip_prefix("shell:") {
        Some(f) => (f, true),
        None => (file, false),
    };
    let path = resolve(cwd, file);
    let contents = std::fs::read_to_string(&path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("failed to read argsfile `{}`: {e}", path.display()),
        )
    })?;
    let words: Vec<String> = if shell {
        shell_split(&contents)?
    } else {
        contents.lines().map(str::to_owned).collect()
    };
    for w in words {
        expand_one(&w, cwd, out, depth + 1)?;
    }
    Ok(())
}

fn resolve(cwd: &Path, file: &str) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        p.to_owned()
    } else {
        cwd.join(p)
    }
}

/// Minimal POSIX-shell-like word splitting (single quotes, double quotes, backslash escapes).
pub fn shell_split(s: &str) -> io::Result<Vec<String>> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err(io::Error::other("unterminated single quote")),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => cur.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                cur.push('\\');
                                cur.push(c);
                            }
                            None => return Err(io::Error::other("unterminated double quote")),
                        },
                        Some(c) => cur.push(c),
                        None => return Err(io::Error::other("unterminated double quote")),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some('\n') => {}
                    Some(c) => cur.push(c),
                    None => {}
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok(words)
}

/// Returns the values of the options named `name`, in either the `--name=value` or the
/// `--name value` form.
pub fn option_values<'a>(args: &'a [String], name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    let mut iter = args.iter();
    std::iter::from_fn(move || {
        while let Some(arg) = iter.next() {
            if arg == name {
                return iter.next().map(String::as_str);
            }
            if let Some(rest) = arg.strip_prefix(name)
                && let Some(value) = rest.strip_prefix('=')
            {
                return Some(value);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nested_argsfiles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("inner"),
            "-Ldependency=a/b\n-Ldependency=c\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("outer.args"),
            "--crate-name=foo\nsrc/lib.rs\n@inner\n--cfg=feature=\"x y\"\n",
        )
        .unwrap();
        let args = s(&["--edition=2021", "@outer.args", "--test"]);
        let expanded = expand_argsfiles(&args, dir.path()).unwrap();
        assert_eq!(
            expanded,
            s(&[
                "--edition=2021",
                "--crate-name=foo",
                "src/lib.rs",
                "-Ldependency=a/b",
                "-Ldependency=c",
                "--cfg=feature=\"x y\"",
                "--test",
            ])
        );
    }

    #[test]
    fn shell_argsfile() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("sh"),
            "--cfg 'feature=\"a b\"' \"--x=\\\"q\\\"\" plain\\ word\n",
        )
        .unwrap();
        let expanded = expand_argsfiles(&s(&["@shell:sh"]), dir.path()).unwrap();
        assert_eq!(
            expanded,
            s(&["--cfg", "feature=\"a b\"", "--x=\"q\"", "plain word"])
        );
    }

    #[test]
    fn missing_argsfile_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = expand_argsfiles(&s(&["@nope"]), dir.path()).unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn options() {
        let args = s(&[
            "--target",
            "x86",
            "--crate-name=foo",
            "--target=y",
            "--targets",
        ]);
        assert_eq!(
            option_values(&args, "--target").collect::<Vec<_>>(),
            ["x86", "y"]
        );
        assert_eq!(
            option_values(&args, "--crate-name").collect::<Vec<_>>(),
            ["foo"]
        );
    }
}
