// SPDX-License-Identifier: MIT
// Port of the original Lua script by Henri Binsztok (2015) to Rust.

use regex::Regex;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn is_comment(line: &str) -> Option<String> {
    let s = line.trim();

    // # comment
    if let Some(c) = Regex::new(r#"^#\s*(.*)$"#).unwrap().captures(s) {
        return Some(c.get(1).unwrap().as_str().to_string());
    }
    // // comment
    if let Some(c) = Regex::new(r#"^//\s*(.*)$"#).unwrap().captures(s) {
        return Some(c.get(1).unwrap().as_str().to_string());
    }
    // <!-- comment -->
    if let Some(c) = Regex::new(r#"^<!--\s*(.*?)\s*-->$"#).unwrap().captures(s) {
        return Some(c.get(1).unwrap().as_str().to_string());
    }
    // (* comment *)
    if let Some(c) = Regex::new(r#"^\(\*\s*(.*?)\s*\*\)$"#).unwrap().captures(s) {
        return Some(c.get(1).unwrap().as_str().to_string());
    }
    None
}

/// Returns (type, command) if the line carries a @build directive
fn detect(line: &str) -> Option<(String, String)> {
    let content = is_comment(line)?;
    let re = Regex::new(r#"@build-?([A-Za-z0-9]*)\s+(.*)$"#).unwrap();
    let caps = re.captures(&content)?;
    let ty = caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
    let cmd = caps.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
    Some((ty, cmd))
}

/// Expand template placeholders the same way as the Lua original:
/// - %<alnum> -> "<alnum>" (quoted)
/// - remaining % -> base (the file stem plus a '.' if the file had an extension)
fn expand_template(template: &str, base: &str) -> String {
    // %<alnum> -> "<alnum>"
    let t = Regex::new(r#"%([A-Za-z0-9]+)"#)
        .unwrap()
        .replace_all(template, r#""$1""#)
        .to_string();

    // remaining lone '%' -> base
    Regex::new(r#"%"#).unwrap().replace_all(&t, base).to_string()
}

/// Read defaults from ~/.config/build.defaults, format:
///   <ext> : <command>
/// Returns the command for the given extension if found.
fn read_defaults(ext: &str) -> Option<String> {
    let path = config_path();
    let path_str = path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.config/build.defaults".to_string());

    let p = match path {
        Some(p) => p,
        None => {
            println!("no default settings found at {}", path_str);
            return None;
        }
    };

    let fh = match File::open(&p) {
        Ok(f) => f,
        Err(_) => {
            println!("no default settings found at {}", path_str);
            return None;
        }
    };

    let re = Regex::new(r#"^([A-Za-z0-9]+)\s*:\s*(.*)$"#).unwrap();
    let want = ext.to_ascii_lowercase();

    for line in BufReader::new(fh).lines().flatten() {
        if let Some(c) = re.captures(&line) {
            let lext = c.get(1).unwrap().as_str().to_ascii_lowercase();
            let lbuild = c.get(2).unwrap().as_str().to_string();
            if lext == want {
                return Some(lbuild);
            }
        }
    }
    None
}

/// Build command runner: expands placeholders then executes via the platform shell.
/// Mirrors `os.execute` behavior by invoking sh -c / cmd /C.
fn run_command(build_tpl: &str, base: &str) -> bool {
    let cmdline = expand_template(build_tpl, base);
    println!("Running: {}", cmdline);

    let status = if cfg!(windows) {
        Command::new("cmd")
            .arg("/C")
            .arg(cmdline)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(cmdline)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    };

    match status {
        Ok(_s) => true, // Lua script returns true after attempting execution, regardless of exit code
        Err(e) => {
            eprintln!("failed to spawn shell: {}", e);
            false
        }
    }
}

/// Returns (base_with_trailing_dot_if_ext, ext_without_dot)
fn base_and_ext(filename: &Path) -> (String, String) {
    let stem = filename
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let ext = filename
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let base = if ext.is_empty() {
        stem
    } else {
        format!("{stem}.")
    };
    (base, ext)
}

fn build_file(type_expected: Option<&str>, filename: &Path) -> bool {
    let fh = match File::open(filename) {
        Ok(f) => f,
        Err(_) => {
            println!("can not read {}", filename.display());
            return false;
        }
    };

    let (base, ext) = base_and_ext(filename);

    // Scan the whole file (the Lua had a TODO to limit to 100 lines; we keep the original behavior)
    for line in BufReader::new(fh).lines().flatten() {
        if let Some((ty, build_tpl)) = detect(&line) {
            let ok_type = match type_expected {
                None => true,
                Some(want) => !ty.is_empty() && ty == want,
            };
            if ok_type && !build_tpl.is_empty() {
                return run_command(&build_tpl, &base);
            }
        }
    }

    // Try defaults if nothing was found inline
    if let Some(default_tpl) = read_defaults(&ext) {
        return run_command(&default_tpl, &base);
    }

    false
}

fn check_build_file(type_expected: Option<&str>, filename: &Path) -> i32 {
    if build_file(type_expected, filename) {
        0
    } else {
        println!("{}: no command found, skipping", filename.display());
        1
    }
}

fn config_path() -> Option<PathBuf> {
    // Follow the Lua’s behavior: HOME + "/.config/build.defaults"
    // No extra crates — minimal dependencies.
    let home = env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".config").join("build.defaults"))
}

fn main() {
    // Mirrors Lua semantics:
    //   - Each "-<type>" flag sets the current build type for subsequent files
    //   - Non-flag args are treated as filenames
    let mut args: Vec<OsString> = env::args_os().collect();

    // Skip argv[0]
    if !args.is_empty() {
        args.remove(0);
    }

    let mut res: i32 = 0;
    let mut ty: Option<String> = None;

    for a in args {
        let s = a.to_string_lossy();
        if s.starts_with('-') && s.len() > 1 {
            let t = s[1..].to_string();
            println!("setting build type: {}", t);
            ty = Some(t);
        } else {
            let path = Path::new(&*s);
            res += check_build_file(ty.as_deref(), path);
        }
    }

    std::process::exit(res);
}
