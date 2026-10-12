//! The licence gate (plan §8.7, §9.5): which tier a pack's translations go to, default-deny.
//!
//! | licence | tier |
//! |---|---|
//! | CC0-1.0, CC-BY-3.0, CC-BY-4.0 | `lib/<pack>/` (committed, credited) |
//! | share-alike, non-commercial, no-derivatives, OGA-BY, GPL, QAL, store licences | `local/<pack>/` |
//! | anything else | refused until someone reads it and adds it here |
//!
//! `lib/` itself is `asset_library::license_allowed`'s; this module adds the local-only list and
//! the text a bundled licence file must hold.

use anyhow::{Result, bail};

use crate::asset_library;

/// Where a translation goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// `lib/<pack>/`: committed and credited.
    Shared,
    /// `local/<pack>/`: this machine only.
    Local,
}

/// Licences (SPDX ids or prefixes, and store licences) whose translations stay on this machine:
/// their terms bind what is made from them (share-alike, attribution in a form the credits file
/// does not give), forbid commercial use or changes, or forbid sharing outside a store.
const LOCAL_ONLY: &[&str] = &[
    "CC-BY-SA-",
    "CC-BY-NC",
    "CC-BY-ND",
    "OGA-BY-",
    "GPL-",
    "LGPL-",
    "AGPL-",
    "QAL",
    "LicenseRef-Mixamo",
    "LicenseRef-Fab",
    "LicenseRef-ShareTextures",
    "LicenseRef-Quaternius-QAL",
];

/// The tier a licence routes to, or why it is refused.
pub fn route(pack: &str, license: &str) -> Result<Route> {
    if pack == pav_core::props::store::CORE {
        bail!("pack core holds reviewed exemplars (asset_edit action=publish): name the import's own pack");
    }
    if asset_library::license_allowed(pack, license).is_ok() {
        return Ok(Route::Shared);
    }
    if LOCAL_ONLY.iter().any(|l| license == l.trim_end_matches('-') || license.starts_with(l)) {
        return Ok(Route::Local);
    }
    bail!("unknown licence {license}: review it, then add it to crates/pav_tools/src/import/licence.rs")
}

/// Words one of which a pack's bundled licence text must contain for its catalog licence: a
/// pack whose licence file no longer says what the catalog claims is not used.
pub fn markers(license: &str) -> &'static [&'static str] {
    match license {
        "CC0-1.0" => &["CC0", "publicdomain/zero", "Creative Commons Zero"],
        l if l.starts_with("CC-BY") => &["creativecommons.org/licenses/by", "Attribution"],
        _ => &[],
    }
}

/// Whether `text` (a pack's licence file) says `license`.
pub fn check_text(license: &str, text: &str) -> Result<()> {
    let m = markers(license);
    if !m.is_empty() && !m.iter().any(|w| text.contains(w)) {
        bail!("the licence file does not say {license} (none of {m:?}): review the pack before using it");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licences_route_by_the_allowlist_and_unknown_ones_are_refused() {
        assert_eq!(route("kenney_castle_kit", "CC0-1.0").unwrap(), Route::Shared);
        assert_eq!(route("some_pack", "CC-BY-4.0").unwrap(), Route::Shared);
        assert_eq!(route("some_pack", "CC-BY-SA-4.0").unwrap(), Route::Local);
        assert_eq!(route("some_pack", "CC-BY-NC-4.0").unwrap(), Route::Local);
        assert_eq!(route("bestiary", "QAL").unwrap(), Route::Local);
        assert_eq!(route("some_pack", "GPL-3.0-only").unwrap(), Route::Local);
        // LIB/core is published, not imported into.
        assert!(route("core", "CC0-1.0").unwrap_err().to_string().contains("publish"));
        let e = route("some_pack", "WTFPL").unwrap_err().to_string();
        assert!(e.contains("unknown licence WTFPL") && e.contains("licence.rs"), "{e}");
        assert!(check_text("CC0-1.0", "License: (Creative Commons Zero, CC0)").is_ok());
        assert!(check_text("CC0-1.0", "All rights reserved").is_err());
        assert!(check_text("LicenseRef-Mixamo", "anything").is_ok());
    }
}
