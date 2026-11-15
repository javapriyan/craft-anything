//! Language catalogs are a folder of TSV files (`build.rs`), combined when the app is built.
//! A pull request that adds strings adds its own file, so it does not collide with another
//! request appending to the same catalog.

#[allow(dead_code)]
#[path = "../build.rs"]
mod build;

use build::{LanguageCatalog, language_catalogs, remove_stale_catalogs};
use std::path::Path;

#[test]
fn catalogs_combine_in_file_name_order_and_a_repeated_key_fails() {
    let dir = tempfile::tempdir().unwrap();
    let de = dir.path().join("de");
    std::fs::create_dir(&de).unwrap();
    // No trailing newline: the next file must not glue onto this line.
    std::fs::write(de.join("b.tsv"), "Close\tZu\n").unwrap();
    std::fs::write(de.join("a.tsv"), "Open\tÖffnen").unwrap();
    let built = language_catalogs(dir.path()).unwrap();
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    assert_eq!(built.languages.len(), 1);
    assert_eq!(built.languages[0].code, "de");
    assert_eq!(built.languages[0].text, "Open\tÖffnen\nClose\tZu\n");
    assert!(built.languages[0].inputs.iter().any(|p| p.ends_with("a.tsv")));

    // The same words twice are still two definitions.
    std::fs::write(de.join("c.tsv"), "Open\tÖffnen\n").unwrap();
    let built = language_catalogs(dir.path()).unwrap();
    let error = built.errors.iter().find(|e| e.contains("Open")).unwrap_or_else(|| panic!("duplicate Open must fail: {:?}", built.errors));
    assert!(error.contains("a.tsv:1") && error.contains("c.tsv:1"), "{error}");
    assert!(!error.contains('\n'), "one line, as Cargo reads it: {error}");

    // A comment and an empty translation are not that key.
    std::fs::write(de.join("c.tsv"), "# Open\tignored\nEmpty\t \nSave\tSpeichern\n").unwrap();
    let built = language_catalogs(dir.path()).unwrap();
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    assert!(built.languages[0].text.contains("Save\tSpeichern\n"), "{}", built.languages[0].text);

    // Twice in the one file the folder already had.
    std::fs::write(de.join("a.tsv"), "Open\tÖffnen\nOpen\tAuf\n").unwrap();
    std::fs::remove_file(de.join("c.tsv")).unwrap();
    let built = language_catalogs(dir.path()).unwrap();
    let error = built.errors.iter().find(|e| e.contains("Open")).unwrap_or_else(|| panic!("a repeated line must fail: {:?}", built.errors));
    assert!(error.contains("a.tsv:1") && error.contains("a.tsv:2"), "{error}");
}

#[test]
fn a_generated_catalog_for_a_removed_language_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("i18n");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("de.tsv"), "Open\tÖffnen\n").unwrap();
    std::fs::write(out.join("gone.tsv"), "stale\n").unwrap();
    std::fs::write(out.join("notes.txt"), "leave me\n").unwrap();
    let keep = [LanguageCatalog { code: "de".into(), text: String::new(), inputs: Vec::new() }];
    remove_stale_catalogs(&out, &keep).unwrap();
    assert!(out.join("de.tsv").is_file());
    assert!(!out.join("gone.tsv").exists(), "a deleted language must not keep its generated catalog");
    assert_eq!(std::fs::read_to_string(out.join("notes.txt")).unwrap(), "leave me\n");
    // A clean build has not created the output directory yet.
    assert!(remove_stale_catalogs(&dir.path().join("missing"), &keep).is_ok());
}

#[test]
fn a_catalog_file_beside_the_language_folders_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("de.tsv"), "Open\tÖffnen\n").unwrap();
    let built = language_catalogs(dir.path()).unwrap();
    let error = built.errors.first().unwrap_or_else(|| panic!("a loose catalog must fail: {:?}", built.errors));
    assert!(error.contains("de.tsv") && error.contains("de/"), "{error}");
    assert!(!error.contains('\n'), "{error}");
}

#[test]
fn embedded_catalogs_are_the_language_folders() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/i18n");
    let built = language_catalogs(&root).unwrap();
    assert!(built.errors.is_empty(), "{:?}", built.errors);
    let mut codes: std::collections::BTreeSet<_> = built.languages.iter().map(|l| l.code.as_str()).collect();
    for lang in craft_anything_ui_egui::i18n::LANGUAGES {
        if lang.code == "en" {
            assert!(lang.source.is_empty());
            assert!(!codes.contains("en"));
            continue;
        }
        let found = built.languages.iter().find(|l| l.code == lang.code).unwrap_or_else(|| panic!("no folder for {}", lang.code));
        assert_eq!(found.text, lang.source, "{}", lang.code);
        assert!(codes.remove(lang.code), "{}", lang.code);
    }
    assert!(codes.is_empty(), "folder with no language row: {codes:?}");
}
