//! v0.37 portable profile backup and restore with strict offline preflight.
//! No profile is installed into the active store automatically. Restoration
//! requires a NEW destination directory and never replaces existing content.
//! FNV checksums prevent accidental corruption, NOT malicious modification.
use crate::{
    profile_inventory,
    profile_store::{self, Identity},
    resolution::Resolution,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};
const HEADER: &str = "GA-PORTABLE-BACKUP|1|";
const MANIFEST: &str = "backup-manifest.txt";
const MAX_ENTRIES: usize = 256;
const MAX_PROFILE_BYTES: usize = 2048;
fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
fn hash(bytes: &[u8]) -> u64 {
    let mut value = 0xcbf29ce484222325_u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    value
}
fn check_directory(dir: &Path) -> io::Result<()> {
    if !dir.is_dir() || dir.symlink_metadata()?.file_type().is_symlink() {
        return Err(err("source must be an ordinary directory, not a link"));
    }
    Ok(())
}
fn output_dir_absent(dir: &Path, source: &Path) -> io::Result<()> {
    if fs::symlink_metadata(dir).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    let parent = dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| err("destination must have a parent"))?;
    check_directory(parent)?;
    // Refuse placing export/restore inside the source tree.
    let source = fs::canonicalize(source)?;
    let parent = fs::canonicalize(parent)?;
    if parent.starts_with(source) {
        return Err(err("destination cannot be inside source"));
    }
    Ok(())
}
struct Entry {
    name: String,
    bytes: Vec<u8>,
    checksum: u64,
}
fn strictly_valid_file(dir: &Path, name: &str) -> io::Result<Entry> {
    let (alias, stick) = profile_inventory::filename(name)
        .ok_or_else(|| err("noncanonical or unexpected profile filename"))?;
    let file = dir.join(name);
    let meta = fs::symlink_metadata(&file)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_PROFILE_BYTES as u64 {
        return Err(err("profile is a link, non-file or too large"));
    }
    let identity = Identity::new(&alias, stick, Resolution::XInput16)?;
    let bytes = fs::read(&file)?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(err("oversized profile"));
    }
    // Validate exactly the bytes that are included in the backup.
    // A second filesystem read could race with a concurrent file change.
    let _ = profile_store::parse(&bytes, &identity)?;
    let checksum = hash(&bytes);
    Ok(Entry {
        name: name.to_owned(),
        bytes,
        checksum,
    })
}
fn get_export_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    check_directory(dir)?;
    let mut names = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    if names.is_empty() {
        return Err(err("no profiles to back up"));
    }
    if names.len() > MAX_ENTRIES {
        return Err(err("profile count above bound"));
    }
    names.sort();
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        let name = name.into_string().map_err(|_| err("non-utf8 entry name"))?;
        entries.push(strictly_valid_file(dir, &name)?);
    }
    Ok(entries)
}
fn read_manifest(src: &Path) -> io::Result<Vec<Entry>> {
    check_directory(src)?;
    let files = fs::read_dir(src)?.collect::<io::Result<Vec<_>>>()?;
    if !(2..=MAX_ENTRIES + 1).contains(&files.len()) {
        return Err(err("empty backup or unexpected entry count"));
    }
    let path = src.join(MANIFEST);
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65536 {
        return Err(err("invalid backup manifest"));
    }
    let content = fs::read_to_string(&path)?;
    let mut lines = content.lines();
    let header = lines.next().ok_or_else(|| err("manifest header absent"))?;
    let count = header
        .strip_prefix(HEADER)
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| (1..=MAX_ENTRIES).contains(n))
        .ok_or_else(|| err("invalid manifest version or count"))?;
    if count + 1 != files.len() {
        return Err(err("manifest directory count mismatch"));
    }
    let mut entries = Vec::with_capacity(count);
    let mut last = String::new();
    for _ in 0..count {
        let line = lines
            .next()
            .ok_or_else(|| err("missing profile in manifest"))?;
        let mut fields = line.split('|');
        let name = fields.next().ok_or_else(|| err("missing name"))?;
        let length = fields
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or_else(|| err("bad profile size"))?;
        let digest = fields
            .next()
            .and_then(|v| u64::from_str_radix(v, 16).ok())
            .ok_or_else(|| err("bad profile checksum"))?;
        if fields.next().is_some()
            || length > MAX_PROFILE_BYTES
            || !last.is_empty() && name <= last.as_str()
        {
            return Err(err("invalid manifest entry order or duplicate"));
        }
        let candidate = strictly_valid_file(src, name)?;
        if candidate.bytes.len() != length || candidate.checksum != digest {
            return Err(err("backup content differs from manifest"));
        }
        last = name.to_owned();
        entries.push(candidate);
    }
    if lines.next().is_some() {
        return Err(err("trailing manifest records"));
    }
    // Even if count matches, unexpected file names must be rejected.
    for item in files {
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| err("non-utf8 backup file"))?;
        if name != MANIFEST && !entries.iter().any(|e| e.name == name) {
            return Err(err("extra file not listed in backup manifest"));
        }
    }
    Ok(entries)
}
fn create_payload(out: &Path, entries: &[Entry]) -> io::Result<()> {
    fs::create_dir(out)?;
    for entry in entries {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out.join(&entry.name))?;
        file.write_all(&entry.bytes)?;
        file.sync_all()?;
    }
    Ok(())
}
#[derive(Clone, Copy, Debug)]
pub struct Summary {
    pub profiles: usize,
    pub bytes: usize,
}
impl Summary {
    pub fn json(self, operation: &str) -> String {
        format!(
            concat!(
                "{{\"schema\":1,\"operation\":\"{}\",",
                "\"profiles\":{},\"payload_bytes\":{},",
                "\"profiles_auto_activated\":false,",
                "\"existing_profiles_overwritten\":false,",
                "\"checksum_is_cryptographic_authentication\":false,",
                "\"hardware_identity_verified\":false,",
                "\"gamepad_writes\":0,\"production\":\"BLOCKED\"}}\n"
            ),
            operation, self.profiles, self.bytes
        )
    }
}
/// Export ONLY a complete and validated profile directory into a brand new
/// backup directory. Unexpected data or a corrupt profile refuses the export.
pub fn export(src: &Path, out: &Path) -> io::Result<Summary> {
    let entries = get_export_entries(src)?;
    output_dir_absent(out, src)?;
    let summary = Summary {
        profiles: entries.len(),
        bytes: entries.iter().map(|x| x.bytes.len()).sum(),
    };
    let mut manifest = format!("{HEADER}{}\n", entries.len());
    for e in &entries {
        manifest.push_str(&format!(
            "{}|{}|{:016x}\n",
            e.name,
            e.bytes.len(),
            e.checksum
        ));
    }
    create_payload(out, &entries)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out.join(MANIFEST))?;
    file.write_all(manifest.as_bytes())?;
    file.sync_all()?;
    Ok(summary)
}
/// Restore into a NEW directory, not the active profile store. Validates
/// every input before creation; no implicit activation or file replacement.
pub fn restore(backup: &Path, out: &Path) -> io::Result<Summary> {
    let entries = read_manifest(backup)?;
    output_dir_absent(out, backup)?;
    let summary = Summary {
        profiles: entries.len(),
        bytes: entries.iter().map(|x| x.bytes.len()).sum(),
    };
    create_payload(out, &entries)?;
    Ok(summary)
}
pub fn run_export(src: &Path, out: &Path) -> io::Result<()> {
    let sum = export(src, out)?;
    println!(
        "PROFILE_BACKUP=PASS {} profiles {} bytes location={}",
        sum.profiles,
        sum.bytes,
        out.display()
    );
    println!("{}", sum.json("export"));
    Ok(())
}
pub fn run_restore(src: &Path, out: &Path) -> io::Result<()> {
    let sum = restore(src, out)?;
    println!(
        "PROFILE_RESTORE=PASS {} profiles {} bytes NEW_DIRECTORY={}",
        sum.profiles,
        sum.bytes,
        out.display()
    );
    println!("{}", sum.json("restore"));
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{profile_store::Stick, reacquire::VerifiedCenter, Vec2};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "ga-v37-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn fixture(root: &Path) {
        fs::create_dir(root).unwrap();
        for (name, stick) in [
            ("MY-PAD-01-left.profile", Stick::Left),
            ("MY-PAD-01-right.profile", Stick::Right),
        ] {
            let id = Identity::new("MY-PAD-01", stick, Resolution::XInput16).unwrap();
            let v = VerifiedCenter::restore_checked(
                Vec2::new(0.24, -0.12),
                Vec2::new(0.0001, 0.0001),
                0.0002,
            )
            .unwrap();
            profile_store::save(&root.join(name), &id, &v).unwrap();
        }
    }
    #[test]
    fn two_stick_roundtrip_never_overwrites_or_activates() {
        let base = temp();
        fs::create_dir(&base).unwrap();
        let src = base.join("input");
        let back = base.join("back");
        let restored = base.join("restored");
        fixture(&src);
        let summary = export(&src, &back).unwrap();
        assert_eq!(summary.profiles, 2);
        assert!(summary
            .json("export")
            .contains("\"profiles_auto_activated\":false"));
        assert_eq!(restore(&back, &restored).unwrap().profiles, 2);
        for name in ["MY-PAD-01-left.profile", "MY-PAD-01-right.profile"] {
            assert_eq!(
                fs::read(src.join(name)).unwrap(),
                fs::read(restored.join(name)).unwrap()
            );
        }
        assert_eq!(
            restore(&back, &restored).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            export(&src, &back).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn corrupt_inputs_do_not_create_destination() {
        let base = temp();
        fs::create_dir(&base).unwrap();
        let src = base.join("input");
        fixture(&src);
        fs::write(src.join("MY-PAD-01-left.profile"), b"corrupt").unwrap();
        let out = base.join("backup");
        assert!(export(&src, &out).is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn modified_or_missing_backup_refuses_restore_without_creating_dir() {
        let base = temp();
        fs::create_dir(&base).unwrap();
        let src = base.join("src");
        fixture(&src);
        let back = base.join("backup");
        export(&src, &back).unwrap();
        fs::write(back.join("MY-PAD-01-right.profile"), b"malformed").unwrap();
        assert!(restore(&back, &base.join("restored")).is_err());
        assert!(!base.join("restored").exists());
        let _ = fs::remove_dir_all(base);
    }
    #[test]
    fn unexpected_files_and_nested_destinations_are_rejected() {
        let base = temp();
        fs::create_dir(&base).unwrap();
        let src = base.join("src");
        fixture(&src);
        fs::write(src.join("other.txt"), b"opaque").unwrap();
        assert!(export(&src, &base.join("backup")).is_err());
        fs::remove_file(src.join("other.txt")).unwrap();
        assert!(export(&src, &src.join("nested")).is_err());
        assert!(!src.join("nested").exists());
        let _ = fs::remove_dir_all(base);
    }
}
