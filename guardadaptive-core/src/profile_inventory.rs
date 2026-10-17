//! v0.33 read-only bounded local profile inventory.
//! Profiles use operator-provided aliases, NOT authenticated USB identifiers.
//! Cataloging never activates calibration or opens any controller/game sink.
use crate::{
    profile_store::{self, Identity, Stick},
    resolution::Resolution,
};
use std::{fs, io, path::Path};
const MAX_ENTRIES: usize = 256;
pub struct Row {
    pub alias: String,
    pub stick: Stick,
    pub valid: bool,
    pub reason: &'static str,
}
#[derive(Default)]
pub struct Inventory {
    pub rows: Vec<Row>,
    pub scanned: usize,
    pub ignored: usize,
    pub valid: usize,
    pub invalid: usize,
}
impl Inventory {
    pub fn json(&self) -> String {
        let rows = self
            .rows
            .iter()
            .map(|r| {
                // Identity validator permits only safe ASCII, so no JSON escaping
                // needed for aliases, but verify first before trusting this.
                format!(
                    "{{\"alias\":\"{}\",\"stick\":\"{}\",\"valid\":{},\"reason\":\"{}\"}}",
                    r.alias,
                    r.stick.name(),
                    r.valid,
                    r.reason
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"schema\":1,\"source\":\"read_only_local_profile_inventory\",",
                "\"scanned\":{},\"valid\":{},\"invalid\":{},\"ignored\":{},",
                "\"rows\":[{}],\"profiles_auto_activated\":false,",
                "\"physical_device_id_authenticated\":false,",
                "\"virtual_gamepad_writes\":false,",
                "\"production_gate\":\"BLOCKED\"}}\n"
            ),
            self.scanned, self.valid, self.invalid, self.ignored, rows
        )
    }
}
/// Undo only the documented encoding applied to operator aliases.
/// Reject unexpected percent sequences rather than guessing filenames.
pub(crate) fn filename(name: &str) -> Option<(String, Stick)> {
    let (raw, stick) = if let Some(alias) = name.strip_suffix("-left.profile") {
        (alias, Stick::Left)
    } else {
        let alias = name.strip_suffix("-right.profile")?;
        (alias, Stick::Right)
    };
    if raw.len() > 384 {
        return None;
    }
    let mut alias = String::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if bytes.get(i..i + 3) == Some(b"%3A") {
                alias.push(':');
                i += 3;
            } else {
                return None;
            }
        } else {
            if !bytes[i].is_ascii() {
                return None;
            }
            alias.push(char::from(bytes[i]));
            i += 1;
        }
    }
    let id = Identity::new(&alias, stick, Resolution::XInput16).ok()?;
    let canonical = format!(
        "{}-{}.profile",
        id.device_id.replace(':', "%3A"),
        stick.name()
    );
    if canonical != name {
        return None;
    }
    Some((alias, stick))
}
pub fn scan(root: &Path) -> io::Result<Inventory> {
    let mut inventory = Inventory::default();
    if !root.exists() {
        return Ok(inventory);
    }
    if !root.is_dir() || root.symlink_metadata()?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid profile inventory root",
        ));
    }
    let mut entries = fs::read_dir(root)?.collect::<io::Result<Vec<_>>>()?;
    if entries.len() > MAX_ENTRIES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "profile directory exceeds bounded inventory limit",
        ));
    }
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            inventory.ignored += 1;
            continue;
        };
        let Some((alias, stick)) = filename(name) else {
            inventory.ignored += 1;
            continue;
        };
        inventory.scanned += 1;
        // file_type checks never traverse links. The load itself is confined
        // to the exact canonical operator filename; no recursive traversal.
        let (valid, reason) = if entry.file_type()?.is_symlink() || !entry.file_type()?.is_file() {
            (false, "link_or_nonfile")
        } else {
            let id = Identity::new(&alias, stick, Resolution::XInput16)?;
            match profile_store::load(&entry.path(), &id) {
                Ok(_) => (true, "checksum_and_calibration_pass"),
                Err(_) => (false, "content_or_quality_rejected"),
            }
        };
        if valid {
            inventory.valid += 1;
        } else {
            inventory.invalid += 1;
        }
        inventory.rows.push(Row {
            alias,
            stick,
            valid,
            reason,
        });
    }
    Ok(inventory)
}
pub fn run(root: &Path, out: &Path) -> io::Result<()> {
    let result = scan(root)?;
    crate::truth_lab::save_new(out, &result.json())?;
    println!(
        "PROFILE INVENTORY: scanned={} valid={} invalid={} ignored={} (NO activation)",
        result.scanned, result.valid, result.invalid, result.ignored
    );
    println!(
        "PHYSICAL_ID=UNVERIFIED GAME_OUTPUT=DISABLED {}",
        out.display()
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{reacquire::VerifiedCenter, Vec2};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ga-v33-inventory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn center() -> VerifiedCenter {
        VerifiedCenter::restore_checked(Vec2::new(0.08, -0.04), Vec2::new(0.0001, 0.0001), 0.0001)
            .unwrap()
    }
    #[test]
    fn recognizes_profiles_but_never_activates_them() {
        let dir = temp();
        fs::create_dir_all(&dir).unwrap();
        let l = Identity::new("Pad:17", Stick::Left, Resolution::XInput16).unwrap();
        let r = Identity::new("Pad:17", Stick::Right, Resolution::XInput16).unwrap();
        profile_store::save(&dir.join("Pad%3A17-left.profile"), &l, &center()).unwrap();
        profile_store::save(&dir.join("Pad%3A17-right.profile"), &r, &center()).unwrap();
        fs::write(dir.join("Pad%3A17-right.profile"), "corrupted").unwrap();
        fs::write(dir.join("ignored.txt"), "unrelated").unwrap();
        let i = scan(&dir).unwrap();
        assert_eq!((i.valid, i.invalid, i.ignored, i.scanned), (1, 1, 1, 2));
        assert_eq!(i.rows[0].alias, "Pad:17");
        assert!(i.json().contains("\"profiles_auto_activated\":false"));
        assert!(i.json().contains("\"production_gate\":\"BLOCKED\""));
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn rejects_ambiguous_filename_and_path_sequences() {
        for f in [
            "../a-left.profile",
            "Pad%252F17-left.profile",
            "Pad%2F17-left.profile",
            "Pad%3a17-left.profile",
            "Pad:17-left.profile",
            "bad.txt",
        ] {
            assert!(filename(f).is_none(), "{f}");
        }
        assert_eq!(filename("Pad%3A17-left.profile").unwrap().0, "Pad:17");
    }
    #[test]
    fn empty_and_missing_inventory_are_nonmutating() {
        let dir = temp();
        assert_eq!(scan(&dir).unwrap().scanned, 0);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(scan(&dir).unwrap().scanned, 0);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn nonreplacing_inventory_report() {
        let root = temp();
        fs::create_dir_all(&root).unwrap();
        let out = root.join("inventory.json");
        run(&root, &out).unwrap();
        let old = fs::read(&out).unwrap();
        assert!(run(&root, &out).is_err());
        assert_eq!(old, fs::read(&out).unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
