//! Optional craft-fonts (fallbacks for CJK, Arabic, Thai, Hebrew, Devanagari…), following
//! craft-fonts' `docs/integration.md`. Releases and CI set it.
//!
//! Unset `CRAFT_FONTS_DIR` (the default) embeds nothing and the app uses its own Figtree/JetBrains
//! Mono. Set it to an absolute path of a craft-fonts checkout to embed every font its manifest
//! lists. A bad path is a warning unless `CRAFT_FONTS_REQUIRED=1`.
//!
//! With `CRAFT_FONTS_REQUIRED` set (to anything but nothing or `0`) the build fails unless the
//! fonts are embedded: for a bad path, and for no `CRAFT_FONTS_DIR` at all. A build that was asked
//! for the fonts never quietly ships without them.
//!
//! It also combines every `src/i18n/<language>/*.tsv` into `$OUT_DIR/i18n/<language>.tsv`, in
//! file-name order. A new UI string goes in its own file in that folder, so two pull requests
//! don't edit the same lines. A key defined in two files of one language fails the build.
//!
//! `tests/craft_fonts_build.rs` reads this file as a module to test [`plan`].
//! `tests/i18n_catalogs.rs` reads it to test [`language_catalogs`].

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32");
    let plan = plan(std::env::var_os("CRAFT_FONTS_DIR"), std::env::var_os("CRAFT_FONTS_REQUIRED"), wasm);
    for input in &plan.inputs {
        println!("cargo::rerun-if-changed={}", input.display());
    }
    if let Some(warning) = &plan.warning {
        println!("cargo::warning={warning}");
    }
    // Cargo fails the build on this line; the table is still written so that is the only error.
    if let Some(error) = &plan.error {
        println!("cargo::error={error}");
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, &plan.source) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
    embed_catalogs();
}

/// What a build does about craft-fonts, decided from its inputs alone.
#[derive(Debug, Default)]
pub struct Plan {
    /// The `CRAFT_FONTS` table `theme.rs` includes; empty when no fonts are embedded.
    pub source: String,
    /// Files the table was made from: the build runs again when one changes.
    pub inputs: Vec<PathBuf>,
    /// Fails the build: the fonts were required and aren't embedded.
    pub error: Option<String>,
    /// The fonts were asked for, not required, and aren't embedded.
    pub warning: Option<String>,
}

/// `dir` and `required` are `CRAFT_FONTS_DIR` and `CRAFT_FONTS_REQUIRED` as the environment has
/// them (set to nothing counts as unset); `wasm` is whether the build is for the browser.
pub fn plan(dir: Option<OsString>, required: Option<OsString>, wasm: bool) -> Plan {
    let required = required.is_some_and(|v| !v.is_empty() && v != "0");
    let dir = dir.filter(|d| !d.is_empty()).map(PathBuf::from);
    let mut plan = Plan::default();
    let mut entries = String::new();
    match dir {
        None if required => {
            plan.error = Some(
                "CRAFT_FONTS_REQUIRED is set but CRAFT_FONTS_DIR is not, so no craft-fonts would be embedded: set CRAFT_FONTS_DIR to the absolute path of a \
                 craft-fonts checkout (docs/localization.md), or unset CRAFT_FONTS_REQUIRED to build without them"
                    .into(),
            );
        }
        None => {}
        Some(dir) => match craft_fonts(&dir, wasm, &mut plan.inputs) {
            Ok(found) if required && found.is_empty() => {
                plan.error = Some(format!("CRAFT_FONTS_DIR={}: no fonts remain after filtering for this target", dir.display()));
            }
            Ok(found) => entries = found,
            Err(e) if required => plan.error = Some(format!("CRAFT_FONTS_DIR={}: {e}", dir.display())),
            Err(e) => plan.warning = Some(format!("building without craft-fonts: CRAFT_FONTS_DIR={}: {e}", dir.display())),
        },
    }
    plan.source = format!("pub static CRAFT_FONTS: &[CraftFont] = &[\n{entries}];\n");
    plan
}

/// One `CraftFont { .. }` initialiser per manifest line; the files read are added to `inputs`.
fn craft_fonts(dir: &Path, wasm: bool, inputs: &mut Vec<PathBuf>) -> Result<String, String> {
    let manifest = dir.join("fonts/manifest.txt");
    inputs.push(manifest.clone());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    const WEB_FONTS: &[(&str, &str)] = &[("BIZ UDPGothic", "Regular")];
    let mut out = String::new();
    let mut listed = 0usize;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = f.as_slice() else {
            return Err(format!("malformed manifest line: {line}"));
        };
        listed += 1;
        if wasm && !WEB_FONTS.contains(&(*family, *style)) {
            continue;
        }
        let path = dir.join(file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        inputs.push(path.clone());
        let scripts: Vec<String> = scripts.split(',').map(|s| format!("{:?}", s.trim())).collect();
        let _ = writeln!(
            out,
            "    CraftFont {{ family: {family:?}, style: {style:?}, scripts: &[{}], bytes: include_bytes!({:?}) }},",
            scripts.join(", "),
            path.display().to_string(),
        );
    }
    if listed == 0 {
        return Err(format!("{} lists no fonts", manifest.display()));
    }
    Ok(out)
}

/// Write one combined catalog per language folder. Cargo fails the build on `errors` but the
/// text is still written, so that message is the only compiler error.
fn embed_catalogs() {
    // The folder itself, so a new language directory or a stray `*.tsv` is noticed.
    println!("cargo::rerun-if-changed=src/i18n");
    let built = match language_catalogs(Path::new("src/i18n")) {
        Ok(built) => built,
        Err(e) => {
            println!("cargo::error={e}");
            return;
        }
    };
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("i18n");
    // A language folder that was removed must not leave its previous catalog for include_str!.
    // A clean build has no output directory yet; that is not an error.
    if let Err(e) = remove_stale_catalogs(&out_dir, &built.languages) {
        println!("cargo::error={e}");
    }
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        println!("cargo::error=creating {}: {e}", out_dir.display());
        return;
    }
    for lang in &built.languages {
        for input in &lang.inputs {
            println!("cargo::rerun-if-changed={}", input.display());
        }
        let dest = out_dir.join(format!("{}.tsv", lang.code));
        if let Err(e) = std::fs::write(&dest, &lang.text) {
            println!("cargo::error=writing {}: {e}", dest.display());
        }
    }
    for error in &built.errors {
        println!("cargo::error={error}");
    }
}

/// Delete generated `*.tsv` files in `out_dir` for languages that are no longer built.
/// Other files are left alone. A missing directory is a clean build and is not an error.
pub fn remove_stale_catalogs(out_dir: &Path, languages: &[LanguageCatalog]) -> Result<(), String> {
    let entries = match std::fs::read_dir(out_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("reading {}: {e}", out_dir.display())),
    };
    for entry in entries {
        let path = entry.map_err(|e| format!("reading {}: {e}", out_dir.display()))?.path();
        if !path.is_file() || path.extension().is_none_or(|ext| ext != "tsv") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if languages.iter().any(|lang| lang.code == stem) {
            continue;
        }
        std::fs::remove_file(&path).map_err(|e| format!("removing stale catalog {}: {e}", path.display()))?;
    }
    Ok(())
}

/// One language folder combined into the catalog text the app embeds.
#[derive(Debug)]
pub struct LanguageCatalog {
    /// BCP 47 code, the folder name.
    pub code: String,
    /// Every `*.tsv` in the folder, in file-name order, still separated into their own lines.
    pub text: String,
    /// The folder and each catalog file. The build runs again when one changes or a file is added.
    pub inputs: Vec<PathBuf>,
}

/// The language folders under `i18n_dir`, and the catalog problems that fail the build.
#[derive(Debug)]
pub struct LanguageCatalogs {
    pub languages: Vec<LanguageCatalog>,
    /// A stray `*.tsv` beside the folders, or a key defined twice. One line each, as Cargo
    /// prints `cargo::error`. An empty language folder is an [`Err`] instead: there is nothing
    /// to embed.
    pub errors: Vec<String>,
}

/// Read `i18n_dir`. Each immediate subdirectory is a language. Its `*.tsv` files are combined
/// in file-name order. Lookup order does not matter; a repeated key is an error so it cannot
/// silently win.
///
/// A key is the same string `i18n::parse` maps: `#` comments and empty translations are not
/// entries, and `\n` in a key is a newline.
pub fn language_catalogs(i18n_dir: &Path) -> Result<LanguageCatalogs, String> {
    let entries = std::fs::read_dir(i18n_dir).map_err(|e| format!("reading {}: {e}", i18n_dir.display()))?;
    let mut dirs = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("reading {}: {e}", i18n_dir.display()))?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            return Err(format!("{} has a file name that is not UTF-8", i18n_dir.display()));
        };
        if path.is_dir() {
            dirs.push((name.to_string(), path));
        } else if path.extension().is_some_and(|ext| ext == "tsv") {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
            errors.push(format!(
                "{name} sits next to the language folders; move it into {}/{stem}/ so a pull request can add its own catalog file",
                i18n_dir.display()
            ));
        }
    }
    dirs.sort();
    let mut languages = Vec::new();
    for (code, dir) in dirs {
        let (text, inputs, faults) = combine_language(&code, &dir)?;
        errors.extend(faults);
        languages.push(LanguageCatalog { code, text, inputs });
    }
    Ok(LanguageCatalogs { languages, errors })
}

/// Combine one language folder. `Err` is an I/O failure. Duplicate keys are returned beside the
/// text so the build can still embed the folder and report only the duplicate.
fn combine_language(code: &str, dir: &Path) -> Result<(String, Vec<PathBuf>, Vec<String>), String> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("reading {}: {e}", dir.display()))?.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "tsv") {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(format!("{code}: {} has no .tsv catalogs", dir.display()));
    }
    let mut inputs = vec![dir.to_path_buf()];
    let mut text = String::new();
    let mut seen: HashMap<String, (String, usize)> = HashMap::new();
    let mut errors = Vec::new();
    for path in files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("catalog.tsv");
        let chunk = std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        // A file that does not end in a newline would glue onto the next file's first line.
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        for (line_no, line) in chunk.lines().enumerate() {
            let Some(key) = catalog_key(line) else { continue };
            let line_no = line_no + 1;
            if let Some((first, first_line)) = seen.get(&key) {
                errors.push(format!("{code}: {key:?} is in {first}:{first_line} and {name}:{line_no}"));
            } else {
                seen.insert(key, (name.to_string(), line_no));
            }
        }
        text.push_str(&chunk);
        inputs.push(path);
    }
    Ok((text, inputs, errors))
}

/// Lookup key of one catalog line. `None` for a comment, a line with no tab, or an empty column.
fn catalog_key(line: &str) -> Option<String> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once('\t')?;
    if key.is_empty() || value.trim().is_empty() {
        return None;
    }
    Some(key.replace("\\n", "\n"))
}
