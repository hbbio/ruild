// SPDX-License-Identifier: MIT
// Port of the original Lua script by Henri Binsztok (2015) to Rust.

use regex::Regex;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::collections::HashMap;

#[cfg(target_os = "macos")]
const BUNDLED_DEFAULTS: &str = include_str!("../defaults/macos.defaults");
#[cfg(all(unix, not(target_os = "macos")))]
const BUNDLED_DEFAULTS: &str = include_str!("../defaults/unix.defaults");
#[cfg(windows)]
const BUNDLED_DEFAULTS: &str = include_str!("../defaults/windows.defaults");

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
    // <!-- comment -->, or start of multiline comment
    if let Some(c) = Regex::new(r#"^<!--\s*(.*?)\s*(?:-->)?\s*$"#)
        .unwrap()
        .captures(s)
    {
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

/// Expand template placeholders like the original script:
/// - %<alnum> -> base + <alnum>, quoted (e.g., base="doc.", %pdf -> "doc.pdf")
/// - remaining lone '%' -> base (no extra quoting)
fn expand_template(template: &str, base: &str) -> String {
    // First, expand %<token> to "{base}{token}"
    let re_tokens = Regex::new(r#"%([A-Za-z0-9]+)"#).unwrap();
    let t = re_tokens
        .replace_all(template, |caps: &regex::Captures| {
            format!("\"{}{}\"", base, &caps[1])
        })
        .to_string();

    // Then replace remaining single '%' with the base as-is
    Regex::new(r#"%"#).unwrap().replace_all(&t, base).to_string()
}

/// Read defaults from ~/.config/build.defaults, format:
///   <ext> : <command>
/// Returns the command for the given extension if found.
fn read_defaults(ext: &str) -> Option<String> {
    let p = config_path()?;

    // Ensure a defaults file exists; if not, bootstrap it.
    if !p.exists() {
        if let Err(e) = ensure_bootstrap_defaults(&p) {
            eprintln!(
                "could not create default settings at {}: {}",
                p.display(),
                e
            );
            return None;
        } else {
            println!("created default settings at {}", p.display());
        }
    }

    let fh = match File::open(&p) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open {}: {}", p.display(), e);
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
fn run_command(build_tpl: &str, base: &str, workdir: &Path, filename: &Path, ty: Option<&str>) -> bool {
    let cmdline = expand_template(build_tpl, base);
    let cmdline = expand_vars(cmdline, filename, workdir, ty);
    println!("Running: {}", cmdline);

    let status = if cfg!(windows) {
        Command::new("cmd")
            .arg("/C")
            .arg(cmdline)
            .current_dir(workdir)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(cmdline)
            .current_dir(workdir)
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

/// Additional variable expansion on top of % placeholders for project-aware rules.
/// Supported variables:
///   {{file}}      -> quoted file name (no path)
///   {{file_stem}} -> quoted file stem
///   {{dir}}       -> quoted directory path of the file
///   {{pm}}        -> chosen package manager (npm|yarn|pnpm|bun)
///   {{pm_start}}  -> pm-specific start command
///   {{pm_test}}   -> pm-specific test command
///   {{pm_install}}-> pm-specific install command
///   {{type}}      -> normalized type (e.g., "build", "down"), empty if none
fn expand_vars(mut s: String, filename: &Path, workdir: &Path, ty: Option<&str>) -> String {
    let file_name = filename.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let file_stem = filename.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let dir_disp = workdir.display().to_string();

    // Resolve package manager from workdir
    let pm = pick_package_manager(workdir);
    let pm_str = match pm { PackageManager::Npm => "npm", PackageManager::Yarn => "yarn", PackageManager::Pnpm => "pnpm", PackageManager::Bun => "bun" };
    let pm_start = match pm { PackageManager::Npm => "npm start", PackageManager::Yarn => "yarn start", PackageManager::Pnpm => "pnpm start", PackageManager::Bun => "bun run start" };
    let pm_test  = match pm { PackageManager::Npm => "npm test",  PackageManager::Yarn => "yarn test",  PackageManager::Pnpm => "pnpm test",  PackageManager::Bun => "bun run test" };
    let pm_install = match pm { PackageManager::Npm => "npm install",  PackageManager::Yarn => "yarn install",  PackageManager::Pnpm => "pnpm install",  PackageManager::Bun => "bun install" };

    let ty_norm = ty.map(normalize_type).unwrap_or_default();

    let replacements = [
        ("{{file}}", format!("\"{}\"", file_name)),
        ("{{file_stem}}", format!("\"{}\"", file_stem)),
        ("{{dir}}", format!("\"{}\"", dir_disp)),
        ("{{pm}}", pm_str.to_string()),
        ("{{pm_start}}", pm_start.to_string()),
        ("{{pm_test}}", pm_test.to_string()),
        ("{{pm_install}}", pm_install.to_string()),
        ("{{type}}", ty_norm),
    ];
    for (k, v) in replacements { s = s.replace(k, &v); }
    s
}

#[derive(Debug, Default)]
struct DefaultsCfg {
    ext_map: HashMap<String, String>,
    file_rules: Vec<FileRule>,
}

#[derive(Debug, Clone)]
struct FileRule { pattern: String, ty: Option<String>, cmd: String }

fn parse_defaults_str(s: &str) -> DefaultsCfg {
    let mut cfg = DefaultsCfg::default();
    let re_ext = Regex::new(r#"^([A-Za-z0-9]+)\s*:\s*(.*)$"#).unwrap();
    let re_file = Regex::new(r#"^file:([^\s:]+)(?:\s+-([A-Za-z0-9_-]+))?\s*:\s*(.*)$"#).unwrap();
    for line in s.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') { continue; }
        if let Some(c) = re_file.captures(t) {
            let pat = c.get(1).unwrap().as_str().to_string();
            let ty = c.get(2).map(|m| normalize_type(m.as_str()));
            let cmd = c.get(3).unwrap().as_str().to_string();
            cfg.file_rules.push(FileRule { pattern: pat, ty, cmd });
            continue;
        }
        if let Some(c) = re_ext.captures(t) {
            let lext = c.get(1).unwrap().as_str().to_ascii_lowercase();
            let lbuild = c.get(2).unwrap().as_str().to_string();
            cfg.ext_map.insert(lext, lbuild);
        }
    }
    cfg
}

fn load_defaults_cfg() -> Option<DefaultsCfg> {
    let p = config_path()?;
    if !p.exists() {
        if let Err(e) = ensure_bootstrap_defaults(&p) {
            eprintln!("could not create default settings at {}: {}", p.display(), e);
            return None;
        }
    }
    let data = std::fs::read_to_string(&p).ok()?;
    Some(parse_defaults_str(&data))
}

fn match_file_rule(cfg: &DefaultsCfg, name: &str, ty: Option<&str>) -> Option<String> {
    let lname = name.to_ascii_lowercase();
    let tnorm = ty.map(normalize_type);
    for r in &cfg.file_rules {
        let mut pat = r.pattern.to_ascii_lowercase();
        let star = pat.ends_with('*');
        if star { pat.pop(); }
        let ok = if star { lname.starts_with(&pat) } else { lname == pat };
        if !ok { continue; }
        match (&r.ty, &tnorm) {
            (None, _) => return Some(r.cmd.clone()),
            (Some(rt), Some(t)) if rt == t => return Some(r.cmd.clone()),
            _ => continue,
        }
    }
    None
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

fn normalize_type(t: &str) -> String {
    t.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
fn compose_cmd(ty: Option<&str>) -> String {
    let t = ty.map(normalize_type);
    match t.as_deref() {
        Some("composedown") | Some("down") | Some("dcdown") => "docker compose down".into(),
        Some("composebuild") | Some("build") | Some("dcbuild") => "docker compose build".into(),
        Some("composepull") | Some("pull") | Some("dcpull") => "docker compose pull".into(),
        Some("composelogs") | Some("logs") | Some("dclogs") => "docker compose logs -f".into(),
        Some("composeps") | Some("ps") | Some("dcps") => "docker compose ps".into(),
        Some("composestop") | Some("stop") | Some("dcstop") => "docker compose stop".into(),
        Some("composestart") | Some("start") | Some("dcstart") => "docker compose start".into(),
        Some("composerestart") | Some("restart") | Some("dcrestart") => "docker compose restart".into(),
        Some("composerecreate") | Some("recreate") => "docker compose up -d --force-recreate".into(),
        Some("composeprune") | Some("prune") => "docker compose down --volumes --remove-orphans".into(),
        _ => "docker compose up -d".into(),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PackageManager { Npm, Yarn, Pnpm, Bun }

fn pick_package_manager(dir: &Path) -> PackageManager {
    // Heuristics based on lockfiles
    let has = |name: &str| dir.join(name).exists();
    if has("pnpm-lock.yaml") { return PackageManager::Pnpm; }
    if has("yarn.lock") { return PackageManager::Yarn; }
    if has("bun.lockb") { return PackageManager::Bun; }
    // package-lock.json implies npm
    PackageManager::Npm
}

#[cfg(test)]
fn pm_script(pm: PackageManager, script: &str) -> String {
    let s = script.to_ascii_lowercase();
    match pm {
        PackageManager::Npm => match s.as_str() {
            "install" => "npm install".into(),
            "start" => "npm start".into(),
            "test" => "npm test".into(),
            _ => format!("npm run {}", s),
        },
        PackageManager::Yarn => match s.as_str() {
            "install" => "yarn install".into(),
            _ => format!("yarn {}", s),
        },
        PackageManager::Pnpm => match s.as_str() {
            "install" => "pnpm install".into(),
            _ => format!("pnpm {}", s),
        },
        PackageManager::Bun => match s.as_str() {
            "install" => "bun install".into(),
            _ => format!("bun run {}", s),
        },
    }
}

#[cfg(test)]
fn project_command_for_file(type_expected: Option<&str>, path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name == "book.toml" {
        return Some("mdbook build".to_string());
    }
    if name == "mkdocs.yml" || name == "mkdocs.yaml" {
        return Some("mkdocs build".to_string());
    }
    if name == "conf.py" {
        return Some("sphinx-build -b html . _build/html".to_string());
    }
    if name.starts_with("doxyfile") {
        let fname = path.file_name()?.to_string_lossy().to_string();
        return Some(format!("doxygen {}", fname));
    }
    if name == "docker-compose.yml" || name == "docker-compose.yaml" || name == "compose.yml" || name == "compose.yaml" {
        return Some(compose_cmd(type_expected));
    }
    if name == "package.json" {
        let dir = path.parent().unwrap_or(Path::new("."));
        let pm = pick_package_manager(dir);
        // Default to build when no type is specified
        let tnorm = type_expected.map(normalize_type);
        let script = match tnorm.as_deref() {
            Some("start") | Some("npmstart") => "start",
            Some("test") | Some("npmtest") => "test",
            Some("build") | Some("npmbuild") => "build",
            Some("lint") | Some("npmlint") => "lint",
            Some("format") | Some("npmformat") | Some("fmt") => "format",
            Some("dev") | Some("npmdev") => "dev",
            Some("clean") | Some("npmclean") => "clean",
            Some("install") | Some("npminstall") => "install",
            _ => "build",
        };
        return Some(pm_script(pm, script));
    }
    None
}

fn append_command_segment(cmd: &mut String, fragment: &str) {
    let part = fragment.trim();
    if part.is_empty() {
        return;
    }
    if !cmd.is_empty() && !cmd.ends_with(' ') {
        cmd.push(' ');
    }
    cmd.push_str(part);
}

fn collect_html_command<I>(mut cmd: String, lines: &mut I) -> String
where
    I: Iterator<Item = std::io::Result<String>>,
{
    while let Some(line_res) = lines.next() {
        let line = match line_res {
            Ok(line) => line,
            Err(_) => break,
        };
        let trimmed = line.trim();
        let (fragment, done) = if let Some(idx) = trimmed.find("-->") {
            (&trimmed[..idx], true)
        } else {
            (trimmed, false)
        };
        append_command_segment(&mut cmd, fragment);
        if done {
            break;
        }
    }
    cmd
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

    // Ensure relative paths in build commands resolve from the file's directory
    let workdir = match std::fs::canonicalize(filename) {
        Ok(abs) => abs.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")),
        Err(_) => filename.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")),
    };

    let mut lines = BufReader::new(fh).lines();

    // Scan the whole file (the Lua had a TODO to limit to 100 lines; we keep the original behavior)
    while let Some(line_res) = lines.next() {
        let line = match line_res {
            Ok(line) => line,
            Err(_) => continue,
        };
        if let Some((ty, mut build_tpl)) = detect(&line) {
            let is_multiline_html = {
                let trimmed = line.trim_start();
                trimmed.starts_with("<!--") && !trimmed.contains("-->")
            };
            if is_multiline_html {
                build_tpl = collect_html_command(build_tpl, &mut lines);
            }
            let ok_type = match type_expected {
                None => true,
                Some(want) => !ty.is_empty() && ty == want,
            };
            if ok_type && !build_tpl.is_empty() {
                return run_command(&build_tpl, &base, &workdir, filename, type_expected);
            }
        }
    }

    // Project-aware fallbacks from config defaults
    if let Some(cfg) = load_defaults_cfg() {
        if let Some(tpl) = match_file_rule(&cfg, filename.file_name().and_then(|s| s.to_str()).unwrap_or(""), type_expected) {
            return run_command(&tpl, &base, &workdir, filename, type_expected);
        }
    }

    // Try defaults if nothing was found inline or via project detection
    if let Some(default_tpl) = read_defaults(&ext) {
        return run_command(&default_tpl, &base, &workdir, filename, type_expected);
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
    // Determine a suitable config file path per platform.
    // Unix/macOS: $XDG_CONFIG_HOME/build.defaults or $HOME/.config/build.defaults
    // Windows: %APPDATA%\build.defaults, falling back to $HOME/.config/build.defaults
    #[cfg(windows)]
    {
        if let Some(appdata) = env::var_os("APPDATA").map(PathBuf::from) {
            return Some(appdata.join("build.defaults"));
        }
        // fallback
        let home = env::var_os("HOME").map(PathBuf::from)?;
        return Some(home.join(".config").join("build.defaults"));
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            return Some(xdg.join("build.defaults"));
        }
        let home = env::var_os("HOME").map(PathBuf::from)?;
        Some(home.join(".config").join("build.defaults"))
    }
}

fn ensure_bootstrap_defaults(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = File::create(path)?;
    use std::io::Write;
    f.write_all(BUNDLED_DEFAULTS.as_bytes())?;
    Ok(())
}

fn short_help() -> String {
    let msg = [
        "ruild — build single files from @build comments",
        "",
        "Usage:",
        "  ruild [-type] <file> [<file> ...]",
        "  ruild --config_file",
        "  ruild --dump_defaults",
        "",
        "Options:",
        "  --config_file   Print the config file location and exit",
        "  --dump_defaults Print bundled defaults for this platform and exit",
        "",
        "Notes:",
        "  - Reads @build or @build-{type} from file comments",
        "  - %<token> -> \"<base><token>\", % -> <base>",
        "  - If no inline command, uses $XDG_CONFIG_HOME/build.defaults",
        "    or ~/.config/build.defaults (Unix/macOS), or %APPDATA%\\build.defaults (Windows)",
        "  - Relative paths resolve from the file’s directory",
        "",
        "See README.md for examples.",
    ];
    msg.join("\n")
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

    if args.is_empty() {
        println!("{}", short_help());
        std::process::exit(0);
    }

    // Handle long options first to avoid conflict with -{type}
    // We only support `--config_file` for now.
    for a in &args {
        let s = a.to_string_lossy();
        if s == "--config_file" {
            match config_path() {
                Some(p) => {
                    if !p.exists() {
                        if let Err(e) = ensure_bootstrap_defaults(&p) {
                            eprintln!("failed to create {}: {}", p.display(), e);
                            std::process::exit(1);
                        }
                    }
                    println!("{}", p.display());
                }
                None => println!("<no-default-path>"),
            }
            std::process::exit(0);
        }
        if s == "--dump_defaults" {
            print!("{}", BUNDLED_DEFAULTS);
            std::process::exit(0);
        }
    }

    let mut res: i32 = 0;
    let mut ty: Option<String> = None;

    for a in args {
        let s = a.to_string_lossy();
        if s.starts_with("--") {
            // Unknown long option; show help and exit with error
            eprintln!("Unknown option: {}\n\n{}", s, short_help());
            std::process::exit(2);
        } else if s.starts_with('-') && s.len() > 1 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn tmp_dir(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        p.push(format!("ruild_test_{}_{}_{}", prefix, std::process::id(), n));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_is_comment_variants() {
        assert_eq!(is_comment("# hello").as_deref(), Some("hello"));
        assert_eq!(is_comment("//  hi").as_deref(), Some("hi"));
        assert_eq!(
            is_comment("<!--  spaced content  -->").as_deref(),
            Some("spaced content")
        );
        assert_eq!(
            is_comment("<!-- just the start of a comment").as_deref(),
            Some("just the start of a comment")
        );
        assert_eq!(is_comment("(* test *)").as_deref(), Some("test"));
        assert!(is_comment("no comment here").is_none());
    }

    #[test]
    fn test_detect_build_directive() {
        assert_eq!(
            detect("# @build echo hi"),
            Some(("".to_string(), "echo hi".to_string()))
        );
        assert_eq!(
            detect("// @build-tex xelatex %md"),
            Some(("tex".to_string(), "xelatex %md".to_string()))
        );
        assert_eq!(
            detect("<!-- @build make-doc"),
            Some(("".to_string(), "make-doc".to_string()))
        );
        assert!(detect("# not a build line").is_none());
    }

    #[test]
    fn test_expand_template() {
        let tpl = "env TEXINPUTS=../template: pandoc -N --pdf-engine xelatex  --template=../template/whitepaper.latex -o ../build/%pdf %md";
        let out = expand_template(tpl, "doc.");
        assert_eq!(
            out,
            "env TEXINPUTS=../template: pandoc -N --pdf-engine xelatex  --template=../template/whitepaper.latex -o ../build/\"doc.pdf\" \"doc.md\""
        );
        let out2 = expand_template("echo %a % %b", "base.");
        assert_eq!(out2, "echo \"base.a\" base. \"base.b\"");
    }

    #[test]
    fn test_base_and_ext() {
        let (b, e) = base_and_ext(Path::new("name.md"));
        assert_eq!(b, "name.");
        assert_eq!(e, "md");
        let (b2, e2) = base_and_ext(Path::new("name"));
        assert_eq!(b2, "name");
        assert_eq!(e2, "");
    }

    #[test]
    fn test_run_command_current_dir() {
        let d = tmp_dir("run_command");
        let marker = d.join("marker.txt");
        assert!(!marker.exists());
        // Command writes to a file in the working directory; ensure it lands in `d`.
        let ok = run_command("echo hi > marker.txt", "base.", &d, &d.join("dummy.txt"), None);
        assert!(ok);
        assert!(marker.exists());
    }

    #[test]
    fn test_build_file_inline_executes_in_file_dir() {
        let d = tmp_dir("inline");
        let file = d.join("doc.md");
        write_file(&file, "<!-- @build echo ok > inside -->\ncontent\n");
        let ok = build_file(None, &file);
        assert!(ok);
        assert!(d.join("inside").exists());
    }

    #[test]
    fn test_build_file_multiline_html_command() {
        let d = tmp_dir("multiline");
        let file = d.join("doc.md");
        write_file(
            &file,
            "<!-- @build echo multi\nline > multiline.txt -->\ncontent\n",
        );
        let ok = build_file(None, &file);
        assert!(ok);
        assert!(d.join("multiline.txt").exists());
    }

    #[test]
    fn test_defaults_used_and_run_in_file_dir() {
        let home = tmp_dir("home");
        let conf = home.join(".config").join("build.defaults");
        write_file(&conf, "md : echo default > from_defaults\n");

        // Set HOME so read_defaults finds our file
        let old_home = env::var_os("HOME");
        unsafe { env::set_var("HOME", &home); }

        let d = tmp_dir("defaults");
        let file = d.join("doc.md");
        write_file(&file, "no directives here\n");
        let ok = build_file(None, &file);
        assert!(ok);
        assert!(d.join("from_defaults").exists());

        // restore HOME
        if let Some(v) = old_home { unsafe { env::set_var("HOME", v); } } else { unsafe { env::remove_var("HOME"); } }
    }

    #[test]
    fn test_short_help_contains_usage() {
        let h = short_help();
        assert!(h.contains("Usage:"));
        assert!(h.contains("ruild [-type] <file>"));
        assert!(h.contains("--config_file"));
        assert!(h.contains("--dump_defaults"));
    }

    #[test]
    fn test_dump_defaults_contains_md_rule() {
        // Ensure bundled defaults have at least a markdown rule
        assert!(BUNDLED_DEFAULTS.contains("md:"));
    }

    #[test]
    fn test_defaults_do_not_use_bare_percent_token() {
        // Ensure no active (non-comment) line uses a standalone % token,
        // which would expand to a trailing dot base path.
        for (i, line) in BUNDLED_DEFAULTS.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') { continue; }
            // Look for a bare % either surrounded by spaces or at ends.
            // Accept %<token> usages.
            let bad = t == "%" || t.contains(" % ") || t.ends_with(" %") || t.starts_with("% ");
            assert!(!bad, "defaults contain bare % token on line {}: {}", i+1, line);
        }
    }

    #[test]
    fn test_bootstrap_defaults_created_and_used() {
        // Point XDG_CONFIG_HOME to a temp dir so we don't touch the real config
        let cfgdir = tmp_dir("xdg");
        let cfgfile = cfgdir.join("build.defaults");
        if cfgfile.exists() { fs::remove_file(&cfgfile).unwrap(); }

        let old_xdg = env::var_os("XDG_CONFIG_HOME");
        unsafe { env::set_var("XDG_CONFIG_HOME", &cfgdir); }

        // File does not exist initially; read_defaults should bootstrap it
        let got = read_defaults("txt");
        assert_eq!(got.as_deref(), Some("pandoc -o %pdf %txt"));
        assert!(cfgfile.exists());

        // restore
        if let Some(v) = old_xdg { unsafe { env::set_var("XDG_CONFIG_HOME", v); } } else { unsafe { env::remove_var("XDG_CONFIG_HOME"); } }
    }

    #[test]
    fn test_project_command_for_file_detection() {
        assert_eq!(
            project_command_for_file(None, Path::new("book.toml")).as_deref(),
            Some("mdbook build")
        );
        assert_eq!(
            project_command_for_file(None, Path::new("mkdocs.yml")).as_deref(),
            Some("mkdocs build")
        );
        assert_eq!(
            project_command_for_file(None, Path::new("mkdocs.yaml")).as_deref(),
            Some("mkdocs build")
        );
        assert_eq!(
            project_command_for_file(None, Path::new("conf.py")).as_deref(),
            Some("sphinx-build -b html . _build/html")
        );
        assert_eq!(
            project_command_for_file(None, Path::new("Doxyfile")).as_deref(),
            Some("doxygen Doxyfile")
        );
        assert_eq!(
            project_command_for_file(None, Path::new("Doxyfile.dev")).as_deref(),
            Some("doxygen Doxyfile.dev")
        );
    }

    #[test]
    fn test_compose_and_package_detection_with_types() {
        // docker-compose mapping
        assert_eq!(
            project_command_for_file(Some("down"), Path::new("docker-compose.yml")).as_deref(),
            Some("docker compose down")
        );
        assert_eq!(
            project_command_for_file(Some("build"), Path::new("compose.yaml")).as_deref(),
            Some("docker compose build")
        );

        // package.json mapping with PM detection
        let d = tmp_dir("pm_detect");
        write_file(&d.join("package.json"), "{\n}\n");
        write_file(&d.join("yarn.lock"), "# lock\n");
        let file = d.join("package.json");
        assert_eq!(
            project_command_for_file(Some("build"), &file).as_deref(),
            Some("yarn build")
        );
        // prefer pnpm if lock exists
        write_file(&d.join("pnpm-lock.yaml"), "lockfileVersion: 9\n");
        assert_eq!(
            project_command_for_file(Some("start"), &file).as_deref(),
            Some("pnpm start")
        );
    }
}
