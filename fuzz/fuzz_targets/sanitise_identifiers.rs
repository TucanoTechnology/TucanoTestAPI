//! Fuzz target: identifier sanitisation never yields a path that escapes.
//!
//! Arbitrary bytes are read as a UTF-8 identifier and pushed through the
//! storage layout. Two things must hold for every input:
//!
//! - anything the layout refuses is refused as a bad component, before a path
//!   is built;
//! - anything it accepts is a single plain name — no separator, no `..`, not
//!   absolute — and every path built from it stays inside the data root.
//!
//! Run it with `cargo fuzz run sanitise_identifiers` from `fuzz/`. See
//! `docs/testing/fuzz-and-property-tests.md` for the full procedure.

#![no_main]

use std::path::{Component, Path};

use libfuzzer_sys::fuzz_target;
use tucano_test::storage::{
    Parent, Resource, attachment_path, case_dir, case_marker, ensure_within, folder_name,
    folder_wire_id, node_folder, revision_marker, step_attachment_path, step_dir, suite_dir,
    validate_component, validate_document_id,
};

/// A name the layout accepted must be one plain path element and nothing else.
fn assert_plain_component(value: &str) {
    assert!(!value.is_empty(), "a sanitised name is never empty");
    assert!(!value.contains('/'), "{value:?} carries a path separator");
    assert!(!value.contains('\\'), "{value:?} carries a path separator");
    assert!(!value.contains('\0'), "{value:?} carries a NUL");
    assert_ne!(value, "..", "a sanitised name is never the parent directory");
    assert_ne!(value, ".", "a sanitised name is never the current directory");
    let as_path = Path::new(value);
    assert!(!as_path.is_absolute(), "{value:?} is an absolute path");
    assert_eq!(
        as_path.components().count(),
        1,
        "{value:?} is not one path element"
    );
}

/// Every path the layout builds must sit inside the data root, and expose only
/// plain components below it.
fn assert_inside_root(root: &Path, path: &Path) {
    assert!(path.starts_with(root), "{path:?} escaped {root:?}");
    assert!(
        ensure_within(root, path).is_ok(),
        "{path:?} is outside {root:?}"
    );
    let relative = path
        .strip_prefix(root)
        .expect("a path built from the root is strippable");
    assert!(
        relative
            .components()
            .all(|component| matches!(component, Component::Normal(_))),
        "{path:?} is not made of plain components"
    );
}

fuzz_target!(|data: &[u8]| {
    let root = std::env::temp_dir();
    let identifier = String::from_utf8_lossy(data).into_owned();

    if validate_component(&identifier).is_ok() {
        assert_plain_component(&identifier);
    }

    for resource in Resource::ALL {
        if let Ok(folder) = node_folder(resource, &identifier) {
            assert_plain_component(folder);
        }
        if validate_document_id(resource, &identifier).is_ok() {
            assert_plain_component(&identifier);
        }
    }

    // A folder name and its wire id are inverses, so a folder recovered from a
    // wire id is exactly the folder that was stored.
    let wire_id = folder_wire_id(&identifier);
    assert_eq!(folder_name(&wire_id), identifier.as_str());

    let project = format!("{identifier}.json");
    let suite = format!("{identifier}.json");
    let parent = Parent::Project(project.clone());
    let suite_parent = Parent::Suite {
        project: project.clone(),
        suite: suite.clone(),
    };

    for parent in [&parent, &suite_parent] {
        if let Ok(path) = case_dir(&root, parent, &identifier) {
            assert_inside_root(&root, &path);
        }
        if let Ok(path) = case_marker(&root, parent, &identifier) {
            assert_inside_root(&root, &path);
        }
        if let Ok(path) = revision_marker(&root, parent, &identifier, 1) {
            assert_inside_root(&root, &path);
        }
        if let Ok(path) = step_dir(&root, parent, &identifier, 0) {
            assert_inside_root(&root, &path);
        }
        if let Ok(path) = attachment_path(&root, parent, &identifier, &identifier) {
            assert_inside_root(&root, &path);
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(identifier.as_str()),
                "an accepted attachment name is kept verbatim"
            );
        }
        if let Ok(path) = step_attachment_path(&root, parent, &identifier, 0, &identifier) {
            assert_inside_root(&root, &path);
        }
    }

    if let Ok(path) = suite_dir(&root, &identifier, &identifier) {
        assert_inside_root(&root, &path);
    }
});
