//! Guards the reproducibility and supply-chain properties of the build.
//!
//! Six things are pinned -- Rust dependencies, the Rust toolchain, the Python
//! code-generation dependencies, the CI actions, the gitleaks binary, and the
//! secretlint command. Each is easy to unpin by accident: someone adds a job
//! and copies a `uses: actions/checkout@v4` from a blog post, or drops
//! `--locked` because a lockfile conflict was annoying. None of that fails
//! visibly; it just quietly restores the "CI went red and nobody changed
//! anything" state this all exists to prevent.
//!
//! These tests read the workflow and config files as text. That is coarse, but
//! it is the property being asserted: no *mutable* reference anywhere, which is
//! a lexical question, not a semantic one.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

/// Every workflow file, as `(path, contents)`.
fn workflows() -> Vec<(String, String)> {
    let dir = repo_root().join(".github/workflows");
    let entries =
        fs::read_dir(&dir).unwrap_or_else(|err| panic!("cannot list {}: {err}", dir.display()));
    let mut found: Vec<(String, String)> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let is_yaml = path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml")
            });
            let name = path.file_name()?.to_str()?.to_owned();
            is_yaml.then(|| (name, fs::read_to_string(&path).ok()))
        })
        .filter_map(|(name, body)| body.map(|body| (name, body)))
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no workflow files found");
    found
}

/// Yields every `uses:` reference in every workflow, as `(file, reference)`.
fn action_references() -> Vec<(String, String)> {
    workflows()
        .into_iter()
        .flat_map(|(name, body)| {
            body.lines()
                .filter_map(|line| {
                    let trimmed = line.trim_start().trim_start_matches("- ").trim();
                    trimmed.strip_prefix("uses:").map(|rest| {
                        // Split off the trailing `# vX.Y.Z` comment; the
                        // reference itself is everything before it.
                        let reference = rest.split('#').next().unwrap_or(rest);
                        (name.clone(), reference.trim().to_owned())
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn is_forty_hex(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

// --- C-1, C-2: actions are pinned to immutable references -----------------

#[test]
fn every_action_is_pinned_to_a_full_commit_sha() {
    // C-1. A tag can be moved by whoever owns the upstream repository; a commit
    // SHA cannot. This is the whole of the "no mutable reference" property.
    let references = action_references();
    assert!(!references.is_empty(), "expected some `uses:` references");

    for (file, reference) in references {
        let Some((action, version)) = reference.split_once('@') else {
            panic!("{file}: `uses: {reference}` has no `@<ref>` at all");
        };
        assert!(
            is_forty_hex(version),
            "{file}: `{action}` is pinned to `{version}`, which is not a \
             40-character commit SHA. Tags and branch names are mutable; \
             resolve the tag to its commit and keep the version as a trailing \
             `# vX.Y.Z` comment so Dependabot can still update it."
        );
    }
}

#[test]
fn no_action_uses_a_tag_or_branch_reference() {
    // C-2. Stated separately from C-1 so the failure names the specific
    // anti-pattern rather than "not 40 hex characters".
    for (file, body) in workflows() {
        for line in body.lines() {
            let trimmed = line.trim_start().trim_start_matches("- ").trim();
            let Some(rest) = trimmed.strip_prefix("uses:") else {
                continue;
            };
            let reference = rest.trim();
            let mutable = reference.contains("@v")
                || reference.ends_with("@main")
                || reference.ends_with("@master");
            assert!(
                !mutable,
                "{file}: `uses: {reference}` points at a mutable tag or branch"
            );
        }
    }
}

#[test]
fn every_pinned_action_keeps_a_version_comment() {
    // Not cosmetic: Dependabot works out the current version from this comment
    // and rewrites the SHA and the comment together. Drop it and the pin
    // silently stops being updated.
    for (file, body) in workflows() {
        for line in body.lines() {
            let trimmed = line.trim_start().trim_start_matches("- ").trim();
            if !trimmed.starts_with("uses:") {
                continue;
            }
            assert!(
                trimmed.contains('#'),
                "{file}: `{trimmed}` has no `# <version>` comment, so Dependabot \
                 cannot tell what version the SHA corresponds to"
            );
        }
    }
}

// --- C-3, C-11, C-12: the Rust lockfile -----------------------------------

#[test]
fn cargo_lock_is_tracked_and_not_ignored() {
    // C-11/C-12. `--locked` in CI is meaningless if the lockfile is not in the
    // repository, and the two are easy to get out of step.
    assert!(
        repo_root().join("Cargo.lock").exists(),
        "Cargo.lock must be committed so `--locked` has something to check against"
    );
    for line in read(".gitignore").lines() {
        assert_ne!(
            line.trim().trim_start_matches('/'),
            "Cargo.lock",
            ".gitignore must not exclude Cargo.lock"
        );
    }
}

#[test]
fn ci_cargo_invocations_are_locked() {
    // C-3. Without `--locked`, cargo silently resolves a *different* dependency
    // graph than the one the change was written against, which is the failure
    // mode committing the lockfile was supposed to remove.
    let ci = read(".github/workflows/ci.yml");
    for line in ci.lines() {
        let trimmed = line.trim();
        let is_build_command = ["cargo clippy", "cargo test", "cargo doc", "cargo check"]
            .iter()
            .any(|cmd| trimmed.contains(cmd));
        if !is_build_command {
            continue;
        }
        assert!(
            trimmed.contains("--locked"),
            "ci.yml: `{trimmed}` must pass --locked"
        );
    }
}

// --- C-4: the toolchain ---------------------------------------------------

#[test]
fn rust_toolchain_is_pinned_to_a_concrete_version() {
    // C-4. `channel = "stable"` moves, and when it moves clippy gains lints
    // that turn `-D warnings` red with no change here. Pinning dependencies
    // while leaving the compiler floating leaves the hole open.
    let toolchain = read("rust-toolchain.toml");
    let channel = toolchain
        .lines()
        .find(|line| line.trim_start().starts_with("channel"))
        .unwrap_or_else(|| panic!("rust-toolchain.toml must set a channel"));
    for moving in ["\"stable\"", "\"beta\"", "\"nightly\""] {
        assert!(
            !channel.contains(moving),
            "rust-toolchain.toml pins {moving}, which is a moving reference; \
             use a concrete version such as \"1.96.0\""
        );
    }
    assert!(
        channel.contains('.'),
        "rust-toolchain.toml channel should be a concrete version, got: {channel}"
    );
}

// --- C-5, C-6: the Python code-generation dependencies --------------------

#[test]
fn python_codegen_requirements_are_fully_pinned_and_hashed() {
    // C-5. The generated tree is compared byte for byte by `codegen-check`, so
    // a transitive dependency shipping a new release can turn that job red on
    // an unrelated change. Hashes additionally guard against an artifact being
    // swapped out under a version that is already pinned.
    let requirements = read("tools/codegen/requirements.txt");
    let mut dependencies = 0;
    for line in requirements.lines() {
        let trimmed = line.trim_end();
        // Dependency lines start at column zero; hashes and comments are
        // indented or prefixed.
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with(' ')
            || trimmed.starts_with('-')
        {
            continue;
        }
        dependencies += 1;
        assert!(
            trimmed.contains("=="),
            "tools/codegen/requirements.txt: `{trimmed}` is not pinned with =="
        );
    }
    assert!(dependencies > 0, "requirements.txt lists no dependencies");
    assert!(
        requirements.contains("--hash=sha256:"),
        "tools/codegen/requirements.txt must be generated with --generate-hashes"
    );
}

#[test]
fn upstream_pin_agrees_between_requirements_and_the_generators() {
    // C-6. `assert_supported_version()` catches this at generation time, but
    // only once someone runs the generator; catching it in the test suite means
    // a mismatched bump fails on the change that introduced it.
    let upstream = read("tools/codegen/upstream.py");
    let pinned = upstream
        .lines()
        .find(|line| line.starts_with("PINNED_VERSION"))
        .and_then(|line| line.split('"').nth(1));
    let Some(pinned) = pinned else {
        panic!("tools/codegen/upstream.py must define PINNED_VERSION")
    };

    let requirements = read("tools/codegen/requirements.in");
    let expected = format!("google-genai=={pinned}");
    assert!(
        requirements.contains(&expected),
        "tools/codegen/requirements.in must pin `{expected}` to match \
         upstream.py's PINNED_VERSION"
    );
}

// --- C-7, C-8, C-9: the externally fetched tools --------------------------

#[test]
fn downloaded_binaries_are_checksum_verified() {
    // C-7. Downloading a binary from a release and executing it unverified
    // means a compromised (or merely re-tagged) release runs arbitrary code
    // with the workflow's permissions.
    let workflow = read(".github/workflows/secret-scan.yml");
    assert!(
        workflow.contains("sha256sum -c"),
        "secret-scan.yml downloads a binary; it must verify the checksum"
    );
    assert!(
        workflow.contains("set -euo pipefail"),
        "secret-scan.yml's download script must fail on the first error \
         rather than letting a failed download reach the next command"
    );
}

#[test]
fn secretlint_is_pinned_to_a_patch_version() {
    // C-8. `@13` resolves to the newest 13.x on every run, so a rule change can
    // start flagging an existing file with nothing changed here.
    let workflow = read(".github/workflows/secret-scan.yml");
    let version = secretlint_version(&workflow)
        .unwrap_or_else(|| panic!("secret-scan.yml must set SECRETLINT_VERSION"));
    assert!(
        version.matches('.').count() >= 2,
        "secretlint must be pinned to a full patch version, got `{version}`"
    );
}

#[test]
fn secretlint_version_matches_between_ci_and_the_commit_hook() {
    // C-9. The hook is the fast feedback and CI is the gate; if they disagree,
    // one of them is training people to ignore it.
    let workflow = read(".github/workflows/secret-scan.yml");
    let hook = read("hooks/pre-commit");

    let ci_version = secretlint_version(&workflow)
        .unwrap_or_else(|| panic!("secret-scan.yml must set SECRETLINT_VERSION"));
    let found = hook
        .lines()
        .find(|line| line.trim_start().starts_with("SECRETLINT_VERSION="))
        .and_then(|line| line.split('"').nth(1));
    let Some(hook_version) = found else {
        panic!("hooks/pre-commit must set SECRETLINT_VERSION=\"<version>\"")
    };

    assert_eq!(
        ci_version, hook_version,
        "secretlint is {ci_version} in CI but {hook_version} in hooks/pre-commit"
    );
}

fn secretlint_version(workflow: &str) -> Option<String> {
    workflow
        .lines()
        .find(|line| line.trim_start().starts_with("SECRETLINT_VERSION:"))
        .and_then(|line| line.split('"').nth(1))
        .map(str::to_owned)
}

// --- C-10: the other half of pinning --------------------------------------

#[test]
fn dependabot_covers_every_pinned_ecosystem() {
    // C-10. Pinning without an update path is how a repository ends up two
    // years behind on a dependency with a known advisory. Each ecosystem that
    // got pinned above needs a corresponding update channel.
    let config = read(".github/dependabot.yml");
    for ecosystem in ["cargo", "github-actions", "pip"] {
        assert!(
            config.contains(&format!("package-ecosystem: \"{ecosystem}\"")),
            "dependabot.yml must cover the `{ecosystem}` ecosystem"
        );
    }
    assert!(
        config.contains("google-genai"),
        "dependabot.yml must exclude google-genai from automatic updates: \
         bumping it requires regenerating the whole generated tree, so an \
         automatic PR would always fail codegen-check"
    );
}

// --- workflow permissions -------------------------------------------------

#[test]
fn workflows_do_not_grant_write_permissions_by_default() {
    // Not in the contract, but adjacent and cheap: a workflow that runs on
    // `pull_request` with write permissions is the classic path to a fork
    // exfiltrating secrets.
    for (file, body) in workflows() {
        assert!(
            body.contains("permissions:"),
            "{file} must declare an explicit `permissions:` block"
        );
        assert!(
            !body.contains("pull_request_target"),
            "{file} uses pull_request_target, which runs untrusted code with \
             repository credentials"
        );
    }
}
