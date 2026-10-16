//! v0.49 authenticated UI snapshot: read-only version marker and profile state.
//! Marker syntax is NOT version integrity, publisher signing, or consent.
//! This module never launches a process, installs drivers, or mutates any file.
use crate::profile_inventory;
use std::{fs, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    pub state: &'static str,
    pub value: Option<String>,
}
impl Marker {
    fn missing() -> Self {
        Self {
            state: "missing",
            value: None,
        }
    }
    fn invalid() -> Self {
        Self {
            state: "invalid_or_unsafe",
            value: None,
        }
    }
    fn json(&self) -> String {
        // Only canonical ASCII version chars from valid_marker can reach value.
        let value = self
            .value
            .as_ref()
            .map_or_else(|| "null".into(), |v| format!("\"{v}\""));
        format!("{{\"state\":\"{}\",\"version\":{}}}", self.state, value)
    }
}
fn valid_marker(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("v0.") else {
        return false;
    };
    let mut parts = rest.split('.');
    let (Some(a), Some(b), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    fn numeric(s: &str) -> bool {
        !s.is_empty()
            && s.len() <= 9
            && (s == "0" || !s.starts_with('0'))
            && s.bytes().all(|c| c.is_ascii_digit())
    }
    numeric(a) && numeric(b)
}
pub fn read_marker(path: &Path) -> Marker {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Marker::missing(),
        Err(_) => return Marker::invalid(),
    };
    if !metadata.file_type().is_file() || metadata.len() > 64 {
        return Marker::invalid();
    }
    let bytes = match fs::read(path) {
        Ok(v) if v.len() <= 64 => v,
        _ => return Marker::invalid(),
    };
    let value = match std::str::from_utf8(&bytes) {
        Ok(v) => v.trim_end_matches(['\n', '\r']),
        _ => return Marker::invalid(),
    };
    if !valid_marker(value) {
        return Marker::invalid();
    }
    Marker {
        state: "valid_syntax_only",
        value: Some(value.to_owned()),
    }
}
#[derive(Debug, Clone)]
pub struct Overview {
    pub active: Marker,
    pub previous: Marker,
    pub install_directory: &'static str,
    pub profile_directory: &'static str,
    pub profiles_scanned: usize,
    pub profiles_valid: usize,
    pub profiles_invalid: usize,
    pub profiles_ignored: usize,
    pub profile_error: bool,
    pub reason: &'static str,
}
fn ordinary_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}
/// Offline read of only two fixed version markers and a bounded flat profile directory.
/// No user-supplied HTTP path or profile name is ever used as a filesystem path.
pub fn inspect(profile_root: &Path) -> Overview {
    let Some(install) = profile_root.parent() else {
        return Overview::unavailable();
    };
    if !ordinary_dir(install) {
        return Overview::unavailable();
    }
    let active = read_marker(&install.join("current.txt"));
    let previous = read_marker(&install.join("previous.txt"));
    let profile_exists = ordinary_dir(profile_root);
    let profile_is_absent =
        fs::symlink_metadata(profile_root).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
    let (profiles_scanned, profiles_valid, profiles_invalid, profiles_ignored, profile_error) =
        if profile_exists {
            match profile_inventory::scan(profile_root) {
                Ok(rows) => (rows.scanned, rows.valid, rows.invalid, rows.ignored, false),
                Err(_) => (0, 0, 0, 0, true),
            }
        } else {
            (0, 0, 0, 0, !profile_is_absent)
        };
    let profile_directory = if profile_exists {
        if profile_error {
            "unreadable_or_unsafe"
        } else {
            "ordinary_directory"
        }
    } else if profile_is_absent {
        "missing"
    } else {
        "unsafe_or_unreadable"
    };
    let reason = if active.state != "valid_syntax_only" {
        "active_marker_missing_or_unsafe"
    } else if previous.state != "valid_syntax_only" {
        "previous_marker_missing_or_unsafe"
    } else if active.value == previous.value {
        "same_version_marker"
    } else if profile_directory != "ordinary_directory" {
        "profile_directory_missing_or_unsafe"
    } else if profiles_scanned == 0 {
        "no_profile_evidence"
    } else if profiles_valid != profiles_scanned || profiles_ignored != 0 {
        "invalid_or_unexpected_profiles"
    } else {
        "version_hash_signature_and_hardware_not_verified_in_ui"
    };
    Overview {
        active,
        previous,
        install_directory: "ordinary_directory",
        profile_directory,
        profiles_scanned,
        profiles_valid,
        profiles_invalid,
        profiles_ignored,
        profile_error,
        reason,
    }
}
impl Overview {
    fn unavailable() -> Self {
        Self {
            active: Marker::missing(),
            previous: Marker::missing(),
            install_directory: "missing_or_unsafe",
            profile_directory: "unavailable",
            profiles_scanned: 0,
            profiles_valid: 0,
            profiles_invalid: 0,
            profiles_ignored: 0,
            profile_error: true,
            reason: "install_directory_missing_or_unsafe",
        }
    }
    pub fn json(&self) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"v49_readonly_ui_recovery_overview\",",
                "\"active\":{},\"previous\":{},",
                "\"install_directory\":\"{}\",\"profile_directory\":\"{}\",",
                "\"profiles_scanned\":{},\"profiles_valid\":{},",
                "\"profiles_invalid\":{},\"profiles_ignored\":{},",
                "\"profile_error\":{},\"reason\":\"{}\",",
                "\"version_integrity_verified\":false,",
                "\"publisher_signature_verified\":false,",
                "\"v48_combined_audit_executed_by_ui\":false,",
                "\"hardware_identity_verified\":false,",
                "\"profile_auto_activated\":false,",
                "\"rollback_authorized\":false,\"install_authorized\":false,",
                "\"virtual_output_enabled\":false,\"game_input_writes\":0,",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            self.active.json(),
            self.previous.json(),
            self.install_directory,
            self.profile_directory,
            self.profiles_scanned,
            self.profiles_valid,
            self.profiles_invalid,
            self.profiles_ignored,
            self.profile_error,
            self.reason
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        profile_store::{self, Identity, Stick},
        reacquire::VerifiedCenter,
        resolution::Resolution,
        Vec2,
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn fixture() -> std::path::PathBuf {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("ga-v49-{}-{}", std::process::id(), n));
        fs::create_dir_all(base.join("profiles")).unwrap();
        base
    }
    fn markers(base: &Path) {
        fs::write(base.join("current.txt"), "v0.34.0").unwrap();
        fs::write(base.join("previous.txt"), "v0.33.0").unwrap();
    }
    #[test]
    fn canonical_markers_only_reject_traversal_and_control_characters() {
        for valid in ["v0.0.0", "v0.34.0", "v0.49.123"] {
            assert!(valid_marker(valid));
        }
        for invalid in [
            "v1.34.0",
            "v0.034.0",
            "v0.34",
            "v0.34.0<script>",
            "v0.34.0\ranything",
            "../v0.33.0",
            "v0.4.3.2",
            "v0.9999999999.0",
            "v0.1.00",
            "v0..1",
            "v0.1.-1",
        ] {
            assert!(!valid_marker(invalid), "{invalid}");
        }
    }
    #[test]
    fn absent_and_malformed_evidence_never_unlocks_output() {
        let root = fixture();
        let profiles = root.join("profiles");
        let empty = inspect(&profiles);
        assert_eq!(empty.reason, "active_marker_missing_or_unsafe");
        assert!(empty.json().contains(r#""rollback_authorized":false"#));
        markers(&root);
        let no_profile = inspect(&profiles);
        assert_eq!(no_profile.reason, "no_profile_evidence");
        fs::write(root.join("current.txt"), "v0.34.0<script>").unwrap();
        let bad = inspect(&profiles);
        assert_eq!(bad.active.state, "invalid_or_unsafe");
        assert!(!bad.json().contains("<script>"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn valid_synthetic_profile_is_still_not_an_install_authorization() {
        let root = fixture();
        markers(&root);
        let id = Identity::new("LAB-PAD-001", Stick::Left, Resolution::XInput16).unwrap();
        let center = VerifiedCenter::restore_checked(
            Vec2::new(0.08, -0.04),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        let path = root.join("profiles/LAB-PAD-001-left.profile");
        profile_store::save(&path, &id, &center).unwrap();
        let snap = inspect(&root.join("profiles"));
        assert_eq!(snap.profiles_valid, 1);
        assert_eq!(
            snap.reason,
            "version_hash_signature_and_hardware_not_verified_in_ui"
        );
        assert!(snap.json().contains(r#""production_gate":"BLOCKED""#));
        assert!(!fs::read(path).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn tampered_and_extra_profiles_denied_without_modification() {
        let root = fixture();
        markers(&root);
        let p = root.join("profiles/LAB-PAD-001-left.profile");
        let bytes = b"corrupted";
        fs::write(&p, bytes).unwrap();
        fs::write(root.join("profiles/not-a-profile.html"), b"<img onerror=1>").unwrap();
        let a = inspect(&root.join("profiles"));
        assert_eq!(a.reason, "invalid_or_unexpected_profiles");
        assert_eq!((a.profiles_invalid, a.profiles_ignored), (1, 1));
        assert_eq!(fs::read(&p).unwrap(), bytes);
        assert!(!a.json().contains("<img"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn existing_but_unsafe_profile_root_is_never_accepted() {
        let root = fixture();
        markers(&root);
        fs::remove_dir(root.join("profiles")).unwrap();
        fs::write(root.join("profiles"), b"not a directory").unwrap();
        let a = inspect(&root.join("profiles"));
        assert_eq!(a.profile_directory, "unsafe_or_unreadable");
        assert_eq!(a.reason, "profile_directory_missing_or_unsafe");
        fs::remove_dir_all(root).unwrap();
    }
}
