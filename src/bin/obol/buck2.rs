//! `obol buck2 <target>`: translate a Buck2 Rust target.
//!
//! How it works (only the `buck2` CLI is needed, no file in the Buck project):
//! 1. Build `<target>[check]` in a dedicated isolation dir with
//!    `-c rust.extra_rustc_flags=<Obol's MIR flags>`. Every crate of the graph, std included when
//!    the toolchain builds it from source (as Meta's does), is compiled metadata-only with full
//!    MIR. `-c buck2.defer_write_actions=false` makes Buck write argsfiles to disk eagerly.
//! 2. `buck2 aquery` gives the target's metadata rustc action and the contents of its argsfile,
//!    with content-hash path segments replaced by `aquery_placeholder`. We find the real argsfile
//!    in `buck-out` by matching the contents, asking Buck to materialize it if needed.
//! 3. The argsfile is a `rustc_action.py` command line; we ask Buck to materialize every
//!    `buck-out` path it mentions (with remote execution, inputs are not downloaded by default).
//! 4. We check that the dependencies' metadata was produced by the same rustc as obol-driver.
//! 5. obol-driver runs on the crate's exact rustc arguments, from the project root, with the
//!    action's environment, in explicit translate mode (`OBOL_MODE=translate`, `OBOL_DEST`).

use anyhow::{Context, Result, bail};
use obol_lib::args::Buck2Opts;
use obol_lib::driver_env::{OBOL_ALLOW_MISSING_MIR, OBOL_ARGS_FILE, OBOL_DEST, OBOL_MODE};
use obol_lib::rustc_action::{RustcAction, buck_out_paths};
use obol_lib::{argsfile, rmeta};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::toolchain;

const PLACEHOLDER: &str = "aquery_placeholder";

struct Buck {
    exe: String,
    isolation_dir: String,
    /// `-c` options and user arguments for build/aquery.
    config: Vec<String>,
    project_root: PathBuf,
}

impl Buck {
    fn cmd(&self, sub: &str) -> Command {
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--isolation-dir").arg(&self.isolation_dir).arg(sub);
        cmd
    }

    /// Run a buck2 subcommand, streaming stderr, and return stdout.
    fn run(&self, sub: &str, args: &[String], with_config: bool) -> Result<String> {
        let mut cmd = self.cmd(sub);
        if with_config {
            cmd.args(&self.config);
        }
        cmd.args(args);
        cmd.stderr(Stdio::inherit());
        log_cmd(&cmd);
        let out = cmd
            .output()
            .with_context(|| format!("failed to run `{}`", self.exe))?;
        if !out.status.success() {
            bail!(
                "`buck2 {sub}` failed ({}){}",
                out.status,
                if sub == "build" {
                    "; see the errors above"
                } else {
                    ""
                }
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn buck_out(&self) -> String {
        format!("buck-out/{}", self.isolation_dir)
    }
}

fn log_cmd(cmd: &Command) {
    let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy()).collect();
    eprintln!(
        "[obol] running: {} {}",
        cmd.get_program().to_string_lossy(),
        args.join(" ")
    );
}

pub fn translate_with_buck2(mut opts: Buck2Opts) -> Result<ExitStatus> {
    opts.obol.validate()?;
    // Buck provides std (built from source with the MIR flags, as explicit sysroot deps): the
    // sysroot built by `obol setup` must not be used here.
    if opts.obol.sysroot.is_some() {
        bail!(
            "`--sysroot` is not supported by `obol buck2`: the standard library comes from the \
             Buck build"
        );
    }
    let user_args = std::mem::take(&mut opts.obol.spread);
    let target = opts.target.clone();
    let check_target = format!("{target}[check]");

    let mut config = vec![
        "-c".to_owned(),
        "buck2.defer_write_actions=false".to_owned(),
    ];
    if !opts.no_mir_flags {
        config.push("-c".to_owned());
        config.push(format!(
            "rust.extra_rustc_flags={}",
            obol_lib::mir_options::rustc_flags().join(" ")
        ));
    }
    config.extend(user_args);

    let mut buck = Buck {
        exe: opts.buck2.clone(),
        isolation_dir: opts.isolation_dir.clone(),
        config,
        project_root: PathBuf::new(),
    };
    let root = buck.run("root", &["--kind".into(), "project".into()], false)?;
    buck.project_root = PathBuf::from(root.trim());

    // 1. Build the target's metadata with full MIR for the whole graph.
    buck.run("build", &[check_target.clone()], true)?;

    // 2. Find the target's rustc action and its argsfile.
    let query = format!("deps(\"{check_target}\", 2)");
    let aquery = buck.run(
        "aquery",
        &[
            query,
            "--json".into(),
            "--output-attribute".into(),
            "^(kind|category|identifier|cmd|contents)$".into(),
        ],
        true,
    )?;
    let action = find_rustc_action(&aquery, &target)?;
    let argsfile = locate_argsfile(&buck, &action)?;
    eprintln!("[obol] rustc argsfile: {}", argsfile.display());

    // 3. Parse it and make sure all the inputs are on disk. Nested argsfiles referenced by the
    // top-level one (e.g. `-Ldependency` symlink dirs) may not be materialized yet (cached or
    // remote actions), and expanding them below would fail: materialize them first.
    let top_level: Vec<String> = std::fs::read_to_string(&argsfile)
        .unwrap_or_default()
        .split_whitespace()
        .map(|t| t.trim_matches(|c| c == '"' || c == '\'').to_owned())
        .collect();
    materialize(&buck, &buck_out_paths(&top_level, &buck.buck_out()))?;
    let lines =
        argsfile::expand_argsfiles(&[format!("@{}", argsfile.display())], &buck.project_root)?;
    let rustc_action = RustcAction::parse(&lines).map_err(anyhow::Error::msg)?;
    let out_dir = tempfile::Builder::new().prefix("obol-buck2-").tempdir()?;
    let mut invocation = rustc_action
        .driver_invocation(&buck.project_root, out_dir.path())
        .map_err(anyhow::Error::msg)?;
    materialize_inputs(&buck, &invocation.args)?;
    let expanded = argsfile::expand_argsfiles(&invocation.args, &buck.project_root)?;

    let crate_name = argsfile::option_values(&expanded, "--crate-name")
        .last()
        .unwrap_or("crate")
        .to_owned();
    let mut is_test = expanded.iter().any(|a| a == "--test");
    if opts.test && !is_test {
        invocation.args.push("--test".to_owned());
        is_test = true;
    }

    // 4. Version check.
    let driver_version = driver_version()?;
    let deps_version = deps_rustc_version(&buck.project_root, &expanded);
    eprintln!(
        "[obol] Buck rustc: {} ({}); obol-driver: {}",
        rustc_action.compiler.join(" "),
        deps_version.as_deref().unwrap_or("unknown version"),
        driver_version
    );
    if let Some(deps_version) = &deps_version
        && *deps_version != driver_version
        && !opts.skip_version_check
    {
        bail!(
            "rustc version mismatch: the Buck build of `{target}` uses `{deps_version}`, but \
             obol-driver is `{driver_version}`.\n\
             obol-driver can only read metadata produced by the exact same compiler. Build Obol \
             with the toolchain Buck uses (e.g. rustup `{}` + `rustc-dev`), or point the Buck \
             project at obol-driver's toolchain.\n\
             (Pass --skip-version-check to try anyway.)",
            deps_version
                .strip_prefix("rustc ")
                .and_then(|v| v.split_whitespace().next())
                .unwrap_or("?")
        );
    }

    // 5. Run obol-driver.
    let dest = match &opts.obol.dest_file {
        Some(d) => std::path::absolute(d)?,
        None => {
            let ext = match opts.obol.format {
                obol_lib::args::OutputFormat::Json => "ullbc",
                obol_lib::args::OutputFormat::Postcard => "ullbc.postcard",
            };
            std::env::current_dir()?.join(format!("{crate_name}.{ext}"))
        }
    };
    let args_file = tempfile::Builder::new()
        .prefix("obol-args-")
        .suffix(".json")
        .tempfile()?;
    std::fs::write(args_file.path(), serde_json::to_string(&opts.obol)?)?;

    let mut cmd = toolchain::driver_cmd()?;
    cmd.current_dir(&buck.project_root);
    cmd.args(&invocation.args);
    // Don't leak cargo/obol state from the caller's environment (in particular `OBOL_SYSROOT`:
    // std comes from the Buck build, not from `obol setup`).
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy();
        if k.starts_with("CARGO_") || (k.starts_with("OBOL_") && k != "OBOL_TOOLCHAIN_IS_IN_PATH") {
            cmd.env_remove(&*k);
        }
    }
    for (k, v) in &invocation.env {
        cmd.env(k, v);
    }
    cmd.env(OBOL_MODE, "translate");
    cmd.env(OBOL_DEST, &dest);
    cmd.env(OBOL_ARGS_FILE, args_file.path());
    if is_test {
        cmd.env("OBOL_BUILDING_TEST", "1");
    }
    if opts.allow_missing_mir {
        cmd.env(OBOL_ALLOW_MISSING_MIR, "1");
    }

    if opts.print_command {
        println!("cd {}", buck.project_root.display());
        for (k, v) in &invocation.env {
            println!("export {k}={}", shell_quote(v));
        }
        println!(
            "export {OBOL_MODE}=translate {OBOL_DEST}={}",
            shell_quote(&dest.display().to_string())
        );
        if is_test {
            println!("export OBOL_BUILDING_TEST=1");
        }
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| shell_quote(&a.to_string_lossy()))
            .collect();
        println!("{} {}", cmd.get_program().to_string_lossy(), args.join(" "));
        return Ok(ExitStatus::default());
    }

    log_cmd(&cmd);
    let status = cmd.status().context("could not run obol-driver")?;
    if status.success() {
        eprintln!("[obol] wrote {}", dest.display());
    }
    Ok(status)
}

/// The relevant bits of the target's metadata-producing rustc action.
#[derive(Debug)]
struct RustcActionInfo {
    /// Project-relative path of the argsfile, with `aquery_placeholder` segments.
    argsfile: String,
    /// The argsfile's contents, with `aquery_placeholder` segments.
    contents: String,
}

/// Find, in `buck2 aquery --json` output, the rustc action of `target`'s `[check]` subtarget.
fn find_rustc_action(aquery_json: &str, target: &str) -> Result<RustcActionInfo> {
    let json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(aquery_json).context("parsing `buck2 aquery` output")?;
    // The configured label of the requested target, from its analysis node (`cell//pkg:name[check]
    // (cfg)`), so that we can pick its own actions among those of its dependencies.
    let analysis_key = json
        .iter()
        .find(|(_, v)| v["kind"] == "analysis")
        .map(|(k, _)| k.clone())
        .with_context(|| format!("`buck2 aquery` returned no analysis node for `{target}`"))?;
    let (label, cfg) = analysis_key
        .split_once(' ')
        .with_context(|| format!("unexpected analysis node `{analysis_key}`"))?;
    let label = label.strip_suffix("[check]").unwrap_or(label);
    let owner = format!("(target: `{label} {cfg}`, id:");
    let own: Vec<_> = json.iter().filter(|(k, _)| k.starts_with(&owner)).collect();

    let rustc = own
        .iter()
        .filter(|(_, v)| v["category"] == "rustc")
        .min_by_key(|(_, v)| match v["identifier"].as_str() {
            Some("check") => 0,
            Some("diag") => 1,
            _ => 2,
        })
        .with_context(|| {
            format!(
                "no rustc action found for `{label}[check]`: is `{target}` a Rust target \
                 (rust_library, rust_binary, rust_test)?"
            )
        })?;
    let cmd = rustc.1["cmd"].as_str().unwrap_or_default();
    let argsfile = cmd
        .trim_matches(|c| c == '[' || c == ']')
        .split(", ")
        .find_map(|a| a.strip_prefix('@'))
        .with_context(|| format!("no argsfile in rustc action command `{cmd}`"))?
        .to_owned();
    let write = own
        .iter()
        .find(|(_, v)| {
            v["category"] == "write"
                && v["identifier"]
                    .as_str()
                    .is_some_and(|id| argsfile.ends_with(&format!("/{id}")))
        })
        .with_context(|| format!("no write action found for argsfile `{argsfile}`"))?;
    let contents = write.1["contents"]
        .as_str()
        .context("argsfile write action has no contents")?
        .to_owned();
    Ok(RustcActionInfo { argsfile, contents })
}

/// Find the on-disk argsfile corresponding to the aquery template: the placeholder segment is the
/// content hash of the file, so we look at all candidates and compare contents.
fn locate_argsfile(buck: &Buck, action: &RustcActionInfo) -> Result<PathBuf> {
    let project_root = &buck.project_root;
    let template = &action.argsfile;
    // Project-relative paths of the candidates, one per content-hash directory.
    let candidates: Vec<String> = match template.split_once(&format!("/{PLACEHOLDER}/")) {
        None => vec![template.clone()],
        Some((dir, rest)) => {
            let entries = std::fs::read_dir(project_root.join(dir))
                .with_context(|| format!("argsfile directory `{dir}` not found"))?;
            entries
                .filter_map(|e| e.ok())
                .map(|e| format!("{dir}/{}/{rest}", e.file_name().to_string_lossy()))
                .collect()
        }
    };
    let find_matches = || -> Vec<PathBuf> {
        candidates
            .iter()
            .map(|c| project_root.join(c))
            .filter(|p| {
                std::fs::read_to_string(p)
                    .is_ok_and(|actual| matches_template(&action.contents, &actual))
            })
            .collect()
    };
    let mut matches = find_matches();
    if matches.is_empty() {
        // The argsfile is an input of the rustc action: when that action is a cache hit, Buck
        // doesn't write it, even with `defer_write_actions=false`. Ask Buck to materialize the
        // candidates (a no-op for paths that aren't artifacts it knows of) and look again.
        materialize(buck, &candidates)?;
        matches = find_matches();
    }
    match matches.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("could not find the argsfile `{template}` in buck-out"),
        // Several argsfiles match the template: older builds of the target left argsfiles in
        // other content-hash directories that differ only in (placeholder) hash segments, i.e.
        // they point at *stale* dependency metadata. The build we just ran (re)wrote the current
        // one, so take the most recently modified.
        many => {
            // Prefer candidates whose referenced buck-out paths all exist: an argsfile can be
            // newer than the build we just ran yet reference dirs that were never materialized.
            let complete: Vec<&PathBuf> = many
                .iter()
                .filter(|p| references_exist(project_root, p))
                .collect();
            let pool: Vec<&PathBuf> = if complete.is_empty() { many.iter().collect() } else { complete };
            let newest = pool
                .into_iter()
                .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
                .unwrap()
                .clone();
            eprintln!(
                "[obol] {} argsfiles match `{template}`; using the newest: {}",
                many.len(),
                newest.display()
            );
            Ok(newest)
        }
    }
}

/// Whether every `buck-out/...` path mentioned in the argsfile at `p` exists.
fn references_exist(project_root: &Path, p: &Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(p) else { return false };
    contents
        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '=' || c == '@' || c == ',')
        .filter(|tok| tok.starts_with("buck-out/"))
        .all(|tok| project_root.join(tok).exists())
}

/// Whether `actual` equals `template` where each `aquery_placeholder` stands for one path segment.
fn matches_template(template: &str, actual: &str) -> bool {
    let mut parts = template.split(PLACEHOLDER);
    let Some(first) = parts.next() else {
        return false;
    };
    let Some(mut rest) = actual.strip_prefix(first) else {
        return false;
    };
    for part in parts {
        let seg_end = rest.find('/').unwrap_or(rest.len());
        if seg_end == 0 {
            return false;
        }
        rest = &rest[seg_end..];
        match rest.strip_prefix(part) {
            Some(r) => rest = r,
            None => return false,
        }
    }
    rest.is_empty()
}

/// Ask Buck to materialize every `buck-out` path referenced by the arguments, including those in
/// nested argsfiles (e.g. the `-Ldependency` symlink directories).
fn materialize_inputs(buck: &Buck, args: &[String]) -> Result<()> {
    let buck_out = buck.buck_out();
    let mut paths = buck_out_paths(args, &buck_out);
    materialize(buck, &paths)?;
    // Second round: argsfiles are now on disk, look inside.
    let expanded = argsfile::expand_argsfiles(args, &buck.project_root)?;
    let more: Vec<String> = buck_out_paths(&expanded, &buck_out)
        .into_iter()
        .filter(|p| !paths.contains(p))
        .collect();
    materialize(buck, &more)?;
    paths.extend(more);
    let missing: Vec<_> = paths
        .iter()
        .filter(|p| !buck.project_root.join(p).exists())
        .collect();
    if !missing.is_empty() {
        bail!("inputs of the rustc action are missing after materialization: {missing:?}");
    }
    Ok(())
}

fn materialize(buck: &Buck, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["materialize".to_owned()];
    args.extend(paths.iter().cloned());
    let mut cmd = buck.cmd("debug");
    cmd.args(&args).current_dir(&buck.project_root);
    cmd.stdout(Stdio::null());
    let status = cmd.status().context("running `buck2 debug materialize`")?;
    if !status.success() {
        bail!("`buck2 debug materialize` failed ({status})");
    }
    Ok(())
}

/// `rustc X.Y.Z (hash date)` of obol-driver, the same string rustc writes in metadata headers.
fn driver_version() -> Result<String> {
    let out = toolchain::driver_cmd()?
        .arg("--version")
        .output()
        .context("running `obol-driver --version`")?;
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.lines().next().unwrap_or_default().trim().to_owned())
}

/// The compiler version that produced the dependencies' metadata: read from the `std` (or any)
/// `--extern` rmeta.
fn deps_rustc_version(project_root: &Path, args: &[String]) -> Option<String> {
    let mut externs: Vec<&str> = argsfile::option_values(args, "--extern")
        .filter_map(|e| e.split_once('=').map(|(_, p)| p))
        .filter(|p| p.ends_with(".rmeta"))
        .collect();
    // Prefer std, which every crate has under Buck's explicit sysroot deps.
    externs.sort_by_key(|p| !p.contains("libstd-"));
    externs.iter().find_map(|p| {
        rmeta::version_from_rmeta_file(&project_root.join(p))
            .ok()
            .flatten()
    })
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_matching() {
        assert!(matches_template(
            "a/aquery_placeholder/b\nc/aquery_placeholder/d",
            "a/123abc/b\nc/9f/d"
        ));
        assert!(!matches_template("a/aquery_placeholder/b", "a/123/c"));
        assert!(!matches_template("a/aquery_placeholder/b", "a//b"));
        assert!(matches_template("no placeholder", "no placeholder"));
    }

    #[test]
    fn rustc_action_from_aquery() {
        let json = r#"{
          "(target: `root//app:app (cfg#1)`, id: `25`)": {"kind": "run", "category": "failure_filter", "identifier": "diag", "cmd": "[python3, ff.py]"},
          "root//app:app[check] (cfg#1)": {"kind": "analysis"},
          "(target: `root//app:app (cfg#1)`, id: `24`)": {"kind": "run", "category": "rustc", "identifier": "diag", "cmd": "[python3, buck-out/x/rustc_action.py, @buck-out/x/app/__app__/aquery_placeholder/XIPMD/app-metadata-fast-diag.args]"},
          "(target: `root//app:app (cfg#1)`, id: `23`)": {"kind": "write", "category": "write", "identifier": "XIPMD/app-metadata-fast-diag.args", "contents": "--env=A=1\nrustc\n--rustc-action-separator\nmain.rs"},
          "(target: `root//mylib:mylib (cfg#1)`, id: `8`)": {"kind": "run", "category": "rustc", "identifier": "check", "cmd": "[python3, rustc_action.py, @buck-out/x/mylib.args]"}
        }"#;
        let info = find_rustc_action(json, "//app:app").unwrap();
        assert_eq!(
            info.argsfile,
            "buck-out/x/app/__app__/aquery_placeholder/XIPMD/app-metadata-fast-diag.args"
        );
        assert!(info.contents.starts_with("--env=A=1"));
    }
}
