//! cargo-rustflags: resolve effective RUSTFLAGS for a cargo target.
//!
//! Uses cargo's own config resolution by running `cargo check` with
//! RUSTC_WRAPPER set to itself. A recursive invocation is detected via
//! `__CARGO_RUSTFLAGS_RECURSIVE` and handles three cases:
//!
//! - `-vV` queries: forwarded to real rustc as-is
//! - `--print=*` probes: forwarded with resolved flags stripped (some flags
//!   like `-Clink-self-contained=+linker` error without `-Zunstable-options`)
//! - Compilation: args are printed to stderr with a marker prefix and the
//!   process exits, aborting the build so we can extract the flags.
//!
//! Usage:
//!   cargo rustflags --target x86_64-unknown-linux-gnu
//!   cargo rustflags --config 'target.x86_64-unknown-linux-gnu.rustflags=["-Clink-arg=-fuse-ld=lld"]'

use std::{
    env, fs,
    path::PathBuf,
    process::{self, Command, Stdio},
};

const MARKER: &str = "CRFLAGS:";
const RECURSIVE_ENV: &str = "__CARGO_RUSTFLAGS_RECURSIVE";

fn main() {
    if env::var_os(RECURSIVE_ENV).is_some() {
        wrapper_mode();
    }

    // When invoked as `cargo rustflags`, argv[1] is "rustflags" — skip it.
    let args: Vec<String> = env::args().collect();
    let args = if args.get(1).map(|s| s.as_str()) == Some("rustflags") {
        &args[2..]
    } else {
        &args[1..]
    };

    let mut target: Option<&str> = None;
    let mut configs: Vec<&str> = Vec::new();
    let mut manifest_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" => {
                i += 1;
                target = Some(args.get(i).expect("--target requires a value"));
            }
            "--config" => {
                i += 1;
                configs.push(args.get(i).expect("--config requires a value"));
            }
            "--manifest-path" => {
                i += 1;
                manifest_path = Some(args.get(i).expect("--manifest-path requires a value"));
            }
            "-h" | "--help" => {
                eprintln!(
                    "Usage: cargo rustflags [--target TRIPLE] [--config KEY=VALUE|PATH]... [--manifest-path PATH]"
                );
                eprintln!();
                eprintln!("Resolve the effective RUSTFLAGS that cargo would pass to rustc.");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  --target TRIPLE            Target triple (e.g. x86_64-unknown-linux-gnu)");
                eprintln!("  --config KEY=VALUE|PATH    Extra cargo config overrides or path to a");
                eprintln!("                             TOML config file (repeatable)");
                eprintln!("  --manifest-path PATH       Path to Cargo.toml (for config resolution context)");
                process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    let configs: Vec<String> = configs.iter().map(|c| resolve_config(c)).collect();
    let config_refs: Vec<&str> = configs.iter().map(|s| s.as_str()).collect();
    match resolve(target, &config_refs, manifest_path) {
        Ok(flags) => {
            if !flags.is_empty() {
                println!("{flags}");
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

/// RUSTC_WRAPPER mode: cargo invokes us as `<wrapper> rustc <args...>`.
/// argv[1] is the real rustc path; argv[2..] are the args cargo would pass.
fn wrapper_mode() -> ! {
    let args: Vec<String> = env::args().collect();
    let rustc = &args[1];
    let rustc_args = &args[2..];

    let has_vv = rustc_args.iter().any(|a| a == "-vV");
    let has_print = rustc_args.iter().any(|a| a.starts_with("--print="));

    // -vV: version query from rustup/cargo — forward unchanged.
    if has_vv {
        let status = Command::new(rustc).args(rustc_args).status().unwrap_or_else(|e| {
            eprintln!("failed to exec rustc: {e}");
            process::exit(1);
        });
        process::exit(status.code().unwrap_or(1));
    }

    // --print=* probes: forward but strip resolved flags (-C/-Z/--cfg) that
    // cargo appends. They're irrelevant for probes and some error without
    // -Zunstable-options.
    if has_print {
        let mut filtered = Vec::new();
        let mut skip_next = false;
        for a in rustc_args {
            if skip_next {
                skip_next = false;
                continue;
            }
            if a.starts_with("-C") || a.starts_with("-Z") || a.starts_with("--cfg=") {
                continue;
            }
            if a == "--cfg" {
                skip_next = true;
                continue;
            }
            filtered.push(a.as_str());
        }
        let status = Command::new(rustc).args(&filtered).status().unwrap_or_else(|e| {
            eprintln!("failed to exec rustc: {e}");
            process::exit(1);
        });
        process::exit(status.code().unwrap_or(1));
    }

    // Compilation: emit all args with marker prefix and abort.
    let joined: String = rustc_args
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("{MARKER}{joined}");
    process::exit(1);
}

fn resolve(
    target: Option<&str>,
    configs: &[&str],
    manifest_path: Option<&str>,
) -> Result<String, String> {
    let tmp = env::temp_dir().join("cargo-rustflags");
    fs::create_dir_all(&tmp).map_err(|e| format!("create tmpdir: {e}"))?;

    // Determine working directory for cargo (controls .cargo/config.toml resolution).
    let work_dir = match manifest_path {
        Some(p) => PathBuf::from(p)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf(),
        None => env::current_dir().map_err(|e| format!("cwd: {e}"))?,
    };

    // Minimal dummy crate for cargo to "compile".
    let dummy = tmp.join("dummy");
    let dummy_src = dummy.join("src");
    fs::create_dir_all(&dummy_src).map_err(|e| format!("create dummy: {e}"))?;
    fs::write(
        dummy.join("Cargo.toml"),
        "[package]\nname=\"d\"\nversion=\"0.0.0\"\nedition=\"2021\"\n",
    )
    .map_err(|e| format!("write Cargo.toml: {e}"))?;
    fs::write(dummy_src.join("lib.rs"), "").map_err(|e| format!("write lib.rs: {e}"))?;

    let self_exe = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;

    let mut cmd = Command::new("cargo");
    cmd.arg("check")
        .arg("--manifest-path")
        .arg(dummy.join("Cargo.toml"))
        .env("RUSTC_WRAPPER", &self_exe)
        .env(RECURSIVE_ENV, "1")
        .current_dir(&work_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    if let Some(t) = target {
        cmd.arg("--target").arg(t);
    }
    for c in configs {
        cmd.arg("--config").arg(c);
    }

    let out = cmd.output().map_err(|e| format!("run cargo: {e}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);

    let args_line = stderr
        .lines()
        .find_map(|l| l.strip_prefix(MARKER))
        .ok_or_else(|| format!("failed to capture rustc args.\nstderr:\n{stderr}"))?;

    // Cargo appends resolved rustflags after its own args. The last cargo-generated
    // arg is `dependency=<path>` (from `-L dependency=...`). Everything after is rustflags.
    let all: Vec<&str> = args_line.split_whitespace().collect();
    let split = all
        .iter()
        .rposition(|a| a.starts_with("dependency="))
        .map(|i| i + 1)
        .unwrap_or(0);

    let flags = all[split..].join(" ");
    let _ = fs::remove_dir_all(&tmp);
    Ok(flags)
}

/// Cargo treats `--config` values containing a path separator or ending in
/// `.toml` as file paths. When we detect a path, resolve it to an absolute
/// path so it works regardless of the working directory we pass to cargo.
fn resolve_config(value: &str) -> String {
    let is_path = value.ends_with(".toml") || value.contains('/') || value.contains('\\');
    if is_path {
        // Attempt to canonicalize; fall back to making it absolute via CWD.
        let p = PathBuf::from(value);
        match p.canonicalize() {
            Ok(abs) => abs.to_string_lossy().into_owned(),
            Err(_) => {
                if p.is_absolute() {
                    value.to_string()
                } else {
                    match env::current_dir() {
                        Ok(cwd) => cwd.join(p).to_string_lossy().into_owned(),
                        Err(_) => value.to_string(),
                    }
                }
            }
        }
    } else {
        value.to_string()
    }
}
