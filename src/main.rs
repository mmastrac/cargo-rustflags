//! cargo-rustflags: resolve effective RUSTFLAGS for a cargo target.
//!
//! Uses cargo's own config resolution by running `cargo check` with
//! RUSTC_WRAPPER set to itself. A recursive invocation is detected via
//! `__CARGO_RUSTFLAGS_RECURSIVE`.

use std::{
    env, fs,
    process::{self, Command, Stdio},
};

const MARKER: &str = "CRFLAGS:";
const MARKER_END: &str = "CRFLAGS_END";
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
    let mut list = false;
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
            "-1" | "--list" => {
                list = true;
            }
            "-h" | "--help" => {
                eprintln!(
                    "Usage: cargo rustflags [OPTIONS] [--config KEY=VALUE|PATH]..."
                );
                eprintln!();
                eprintln!("Resolve the effective RUSTFLAGS that cargo would pass to rustc.");
                eprintln!();
                eprintln!("Options:");
                eprintln!("  --target TRIPLE            Target triple (e.g. x86_64-unknown-linux-gnu)");
                eprintln!("  --config KEY=VALUE|PATH    Extra cargo config overrides or path to a");
                eprintln!("                             TOML config file (repeatable)");
                eprintln!("  -1, --list                 Print one flag per line");
                process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    match resolve(target, &configs) {
        Ok(flags) => {
            if !flags.is_empty() {
                if list {
                    for flag in &flags {
                        println!("{flag}");
                    }
                } else {
                    println!("{}", flags.join(" "));
                }
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
    // -Zunstable-options. Also strip `-` (stdin marker) and empty args that
    // cargo may pass in cross-compilation probes — rustc rejects multiple
    // input filenames.
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
            // Strip stdin marker and empty args that cargo passes in probes
            if a == "-" || a.is_empty() {
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

    // Compilation: emit args between sentinels, one per line, to preserve boundaries.
    eprintln!("{MARKER}");
    for arg in rustc_args {
        eprintln!("{arg}");
    }
    eprintln!("{MARKER_END}");
    process::exit(1);
}

fn resolve(target: Option<&str>, configs: &[&str]) -> Result<Vec<String>, String> {
    let tmp = env::temp_dir().join("cargo-rustflags");
    fs::create_dir_all(&tmp).map_err(|e| format!("create tmpdir: {e}"))?;

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

    // Collect args emitted between MARKER and MARKER_END sentinels, one per line.
    let mut in_marker = false;
    let mut all: Vec<&str> = Vec::new();
    for line in stderr.lines() {
        if line == MARKER {
            in_marker = true;
            continue;
        }
        if line == MARKER_END {
            break;
        }
        if in_marker {
            all.push(line);
        }
    }
    if all.is_empty() {
        return Err(format!("failed to capture rustc args.\nstderr:\n{stderr}"));
    }

    // Cargo appends resolved rustflags after its own args. The last cargo-generated
    // arg is `dependency=<path>` (from `-L dependency=...`). Everything after is rustflags.
    let split = all
        .iter()
        .rposition(|a| a.starts_with("dependency="))
        .map(|i| i + 1)
        .unwrap_or(0);

    let flags: Vec<String> = all[split..].iter().map(|s| s.to_string()).collect();
    let _ = fs::remove_dir_all(&tmp);
    Ok(flags)
}
