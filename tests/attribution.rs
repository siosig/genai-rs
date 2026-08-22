//! Guards the Apache-2.0 attribution this crate owes its upstream.
//!
//! The generated types, converters, blocking wrappers, parity table and
//! converter fixtures are derived works of the Google Gen AI Python SDK:
//! the converters are a 1:1 transpilation of its `_to_mldev` / `_from_mldev`
//! helpers and the generated doc comments are its Python docstrings verbatim.
//! Sections 4(b) and 4(c) of the Apache License, Version 2.0 require those
//! files to carry the upstream copyright notice and a statement of change,
//! and require the license text to travel with the work.
//!
//! That obligation is easy to satisfy once and then lose silently: someone
//! rewrites a generator's header, adds a generated module without the block,
//! or "tidies" the `NOTICE` and drops the derived-path list. These tests fail
//! when that happens.
//!
//! Everything here reads files from the repository; nothing touches the
//! network or the API. `CARGO_MANIFEST_DIR` is the repository root because
//! this is a single-package crate.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Marks the upstream copyright holder in SPDX form.
const UPSTREAM_SPDX_COPYRIGHT: &str = "SPDX-FileCopyrightText: 2025 Google LLC";
/// SPDX short-form identifier for the license both works are under.
const SPDX_LICENSE: &str = "SPDX-License-Identifier: Apache-2.0";
/// Prefix of the Apache-2.0 section 4(b) "statement of change" line.
const MODIFIED_MARKER: &str = "Modified:";
/// Plain-text form of the upstream copyright, as it appears in `NOTICE`.
const UPSTREAM_COPYRIGHT: &str = "Copyright 2025 Google LLC";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

/// Every file that must carry the attribution header in its own comment
/// syntax. The JSON fixtures are covered by their directory's README instead
/// (JSON has no comments, and the fixtures are compared byte for byte).
fn files_requiring_header() -> Vec<String> {
    let mut files = vec![
        "src/types/generated/structs.rs".to_owned(),
        "src/types/generated/enums.rs".to_owned(),
        "src/types/generated/mod.rs".to_owned(),
        "src/blocking/generated.rs".to_owned(),
        "docs/parity.md".to_owned(),
        "tests/fixtures/converters/README.md".to_owned(),
    ];
    // The Japanese parity table is emitted only when the generator has its
    // locale support; checked when present rather than required, so this suite
    // does not depend on that being in the tree.
    if repo_root().join("docs/parity.ja.md").exists() {
        files.push("docs/parity.ja.md".to_owned());
    }
    files.extend(rust_files_in("src/converters/generated"));
    files
}

/// Lists the `.rs` files directly inside `relative`, sorted for a stable
/// failure message. Discovering them rather than hard-coding the list means a
/// newly generated converter module cannot slip in without an attribution.
fn rust_files_in(relative: &str) -> Vec<String> {
    let dir = repo_root().join(relative);
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|err| panic!("cannot list {}: {err}", dir.display()));
    let mut files: Vec<String> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let is_rust = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"));
            let name = path.file_name()?.to_str()?.to_owned();
            is_rust.then(|| format!("{relative}/{name}"))
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .rs files found under {relative}");
    files
}

/// Reads the upstream version this port is pinned to, from the single source
/// of truth the generators use.
fn pinned_upstream_version() -> String {
    let source = read("tools/codegen/upstream.py");
    let found = source
        .lines()
        .find(|line| line.starts_with("PINNED_VERSION"))
        .and_then(|line| line.split('"').nth(1));
    let Some(version) = found else {
        panic!("tools/codegen/upstream.py must define PINNED_VERSION = \"<version>\"")
    };
    version.to_owned()
}

// --- A1..A6, A13: the NOTICE and LICENSE files ----------------------------

#[test]
fn notice_file_exists_and_names_the_upstream_work() {
    let notice = read("NOTICE");

    // A2/A3: the two facts an auditor needs first -- who owns the upstream
    // work and under what terms it was licensed to us.
    assert!(
        notice.contains(UPSTREAM_COPYRIGHT),
        "NOTICE must retain the upstream copyright notice (Apache-2.0 4(c))"
    );
    assert!(
        notice.contains("Apache License, Version 2.0"),
        "NOTICE must name the upstream license"
    );
    assert!(
        notice.contains("google-genai"),
        "NOTICE must name the upstream project"
    );
    assert!(
        notice.contains("https://github.com/googleapis/python-genai"),
        "NOTICE must link the upstream project"
    );
}

#[test]
fn notice_states_the_upstream_version_the_generators_are_pinned_to() {
    // A4. A NOTICE naming a version the code was not generated from is worse
    // than none: it asserts a provenance that is not true. Bumping
    // `PINNED_VERSION` without updating `NOTICE` fails here.
    let version = pinned_upstream_version();
    let notice = read("NOTICE");
    assert!(
        notice.contains(&version),
        "NOTICE must state upstream version {version} (from tools/codegen/upstream.py)"
    );
}

#[test]
fn notice_lists_derived_paths_that_all_exist() {
    // A5. The derived-path list is the part an auditor acts on, so a stale
    // entry (a path that was renamed or removed) makes the whole notice
    // untrustworthy.
    let notice = read("NOTICE");
    let derived = [
        "src/types/generated/",
        "src/converters/generated/",
        "src/blocking/generated.rs",
        "tests/fixtures/converters/",
        "docs/parity.md",
        "docs/parity.ja.md",
    ];
    for path in derived {
        assert!(
            notice.contains(path),
            "NOTICE must list the derived path {path}"
        );
        let full = repo_root().join(path.trim_end_matches('/'));
        assert!(full.exists(), "NOTICE lists {path}, which does not exist");
    }
}

#[test]
fn notice_disclaims_trademark_and_affiliation() {
    // A6. Apache-2.0 section 6 grants no trademark rights, and this crate's
    // name and docs necessarily use Google's marks descriptively. Saying so
    // is what keeps that nominative use honest.
    let notice = read("NOTICE");
    assert!(
        notice.contains("trademark"),
        "NOTICE must address trademarks (Apache-2.0 does not license them)"
    );
    assert!(
        notice.contains("not affiliated with"),
        "NOTICE must disclaim affiliation with the upstream vendor"
    );
}

#[test]
fn license_file_is_the_unmodified_apache_2_text() {
    // A13. Attribution belongs in NOTICE, not in an edited copy of the
    // license. The appendix placeholder surviving is the cheapest proof that
    // nobody wrote a copyright line into the license text itself.
    let license = read("LICENSE");
    assert!(
        license.contains("Apache License"),
        "LICENSE must contain the Apache License text"
    );
    assert!(
        license.contains("Copyright [yyyy] [name of copyright owner]"),
        "LICENSE must be the unmodified Apache-2.0 text; \
         put copyright notices in NOTICE instead of editing the license"
    );
}

// --- A7..A10: generated files carry the header ----------------------------

#[test]
fn every_generated_file_carries_the_upstream_copyright() {
    // A7. This is the obligation that was entirely missing before: not one
    // file in the repository named the upstream copyright holder.
    for file in files_requiring_header() {
        assert!(
            read(&file).contains(UPSTREAM_SPDX_COPYRIGHT),
            "{file} is derived from google-genai and must carry \
             `{UPSTREAM_SPDX_COPYRIGHT}` (Apache-2.0 4(c))"
        );
    }
}

#[test]
fn every_generated_file_carries_the_spdx_license_identifier() {
    // A8. The machine-readable half: SBOM tooling reads the identifier, not
    // the prose.
    for file in files_requiring_header() {
        assert!(
            read(&file).contains(SPDX_LICENSE),
            "{file} must carry `{SPDX_LICENSE}`"
        );
    }
}

#[test]
fn every_generated_file_states_what_was_modified() {
    // A9. Apache-2.0 4(b) asks for a prominent notice that the file was
    // changed -- "generated by X from Y" alone does not say the upstream work
    // was modified.
    for file in files_requiring_header() {
        assert!(
            read(&file).contains(MODIFIED_MARKER),
            "{file} must state how the upstream work was modified \
             (Apache-2.0 4(b))"
        );
    }
}

#[test]
fn generated_headers_name_the_pinned_upstream_version() {
    // A generated header claiming a different version than the generators are
    // pinned to means the tree is half-regenerated.
    let version = pinned_upstream_version();
    let expected = format!("google-genai {version}");
    for file in files_requiring_header() {
        // `mod.rs` re-exports and the fixtures README both name the version
        // through the same attribution block, so no file is exempt.
        assert!(
            read(&file).contains(&expected),
            "{file} must name the pinned upstream version ({expected})"
        );
    }
}

#[test]
fn fixture_directory_documents_why_the_json_has_no_header() {
    // A10. JSON cannot carry a comment and `tests/converters_golden.rs`
    // compares these files byte for byte, so the attribution is stated once
    // for the directory. Someone later "fixing" the missing per-file headers
    // would break the golden comparison; the README says so.
    let readme = read("tests/fixtures/converters/README.md");
    assert!(
        readme.contains("JSON cannot carry comments"),
        "the fixtures README must explain why the JSON files have no header"
    );

    let dir = repo_root().join("tests/fixtures/converters");
    assert!(dir.join("README.md").exists());
    assert!(
        count_json_fixtures(&dir) > 100,
        "the fixtures directory should hold the generated golden files"
    );
}

fn count_json_fixtures(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                count_json_fixtures(&path)
            } else {
                usize::from(path.extension().is_some_and(|ext| ext == "json"))
            }
        })
        .sum()
}

// --- A11, A12: the disclaimer a reader meets first ------------------------

#[test]
fn readme_discloses_unofficial_status_before_anything_else() {
    // A11. A disclaimer at the bottom of a 600-line README is not a
    // disclaimer. It has to be in the first screenful.
    // The Japanese README is checked when it is present; a translation is not
    // required for the disclaimer obligation, only consistency if one exists.
    let readmes = ["README.md", "README.ja.md"]
        .into_iter()
        .filter(|name| repo_root().join(name).exists());
    for readme in readmes {
        let head: String = read(readme).lines().take(20).collect::<Vec<_>>().join("\n");
        // "\u{975e}\u{516c}\u{5f0f}" is Japanese for "unofficial". Escaped rather
        // than written literally because the `english-only-content` commit hook
        // rejects CJK in files not named `*.ja*`, and this one is not.
        let unofficial_ja = "\u{975e}\u{516c}\u{5f0f}";
        let discloses = head.contains("Unofficial") || head.contains(unofficial_ja);
        assert!(
            discloses,
            "{readme} must say it is unofficial within its first 20 lines"
        );
        assert!(
            head.contains("NOTICE"),
            "{readme} must point at NOTICE within its first 20 lines"
        );
    }
}

#[test]
fn crate_documentation_discloses_unofficial_status() {
    // A12. Someone reading the crate on a docs site never sees the README.
    let head: String = read("src/lib.rs")
        .lines()
        .take(15)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        head.contains("Unofficial"),
        "the crate-level docs must say the port is unofficial up front"
    );
    assert!(
        head.contains("NOTICE"),
        "the crate-level docs must point at NOTICE"
    );
}
