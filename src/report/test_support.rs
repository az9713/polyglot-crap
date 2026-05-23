//! Test fixtures shared across report submodule test blocks.

use crate::merge::CrapEntry;
use std::path::PathBuf;

/// Two-entry fixture: one trivially clean function and one egregiously
/// crappy function. Used by the json / human / github / dispatcher tests.
pub(crate) fn sample() -> Vec<CrapEntry> {
    vec![
        CrapEntry {
            file: PathBuf::from("a.rs"),
            function: "clean".into(),
            line: 1,
            cyclomatic: 1.0,
            coverage: Some(100.0),
            crap: 1.0,
            crate_name: None,
        },
        CrapEntry {
            file: PathBuf::from("a.rs"),
            function: "crappy".into(),
            line: 10,
            cyclomatic: 10.0,
            coverage: Some(0.0),
            crap: 110.0,
            crate_name: None,
        },
    ]
}
