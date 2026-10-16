//! Per-device, per-stick verified neutral snapshots.
//! FNV-1a64 is an accidental-corruption detector, NOT authentication or tamper
//! resistance. Never load profiles for a different physical identity/stick.
//! Persist only after explicit user consent and completed guided verification.
use crate::{reacquire::VerifiedCenter, resolution::Resolution, Vec2};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
const HEADER: &str = "GA-NEUTRAL";
const VERSION: &str = "1";
const MAX_BYTES: u64 = 2048;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stick {
    Left,
    Right,
}
impl Stick {
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Identity {
    pub device_id: String,
    pub stick: Stick,
    pub resolution: Resolution,
}
impl Identity {
    pub fn new(device_id: &str, stick: Stick, resolution: Resolution) -> io::Result<Self> {
        if !(6..=128).contains(&device_id.len())
            || !device_id.bytes().all(|c| {
                c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.' || c == b':'
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid device identity",
            ));
        }
        Ok(Self {
            device_id: device_id.to_string(),
            stick,
            resolution,
        })
    }
}
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
fn fnv64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325_u64;
    for c in bytes {
        h ^= u64::from(*c);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
fn serialize(identity: &Identity, v: &VerifiedCenter) -> String {
    let center = v.center();
    let payload = format!(
        "{HEADER}|{VERSION}|{}|{}|{}|{:016x}|{:016x}|{:016x}|{:016x}|{:016x}",
        identity.device_id,
        identity.stick.name(),
        identity.resolution.name(),
        center.x.to_bits(),
        center.y.to_bits(),
        v.neutral_mad.x.to_bits(),
        v.neutral_mad.y.to_bits(),
        v.agreement.to_bits()
    );
    format!("{payload}|{:016x}\n", fnv64(payload.as_bytes()))
}
pub(crate) fn parse(bytes: &[u8], identity: &Identity) -> io::Result<VerifiedCenter> {
    if bytes.len() > MAX_BYTES as usize {
        return Err(invalid("profile exceeds maximum byte size"));
    }
    let s = std::str::from_utf8(bytes).map_err(|_| invalid("invalid profile utf8"))?;
    let s = s
        .strip_suffix('\n')
        .ok_or_else(|| invalid("profile missing terminator"))?;
    let fields: Vec<_> = s.split('|').collect();
    if fields.len() != 11 || fields[0] != HEADER || fields[1] != VERSION {
        return Err(invalid("unsupported profile schema"));
    }
    if fields[2] != identity.device_id
        || fields[3] != identity.stick.name()
        || fields[4] != identity.resolution.name()
    {
        return Err(invalid("profile device/stick/resolution mismatch"));
    }
    let expected = u64::from_str_radix(fields[10], 16).map_err(|_| invalid("bad digest"))?;
    let raw = s
        .rsplit_once('|')
        .ok_or_else(|| invalid("invalid profile payload"))?
        .0;
    if expected != fnv64(raw.as_bytes()) {
        return Err(invalid("corrupt profile checksum"));
    }
    let mut parsed = [0_f64; 5];
    for i in 0..5 {
        if fields[i + 5].len() != 16 {
            return Err(invalid("bad numeric bits"));
        }
        parsed[i] = f64::from_bits(
            u64::from_str_radix(fields[i + 5], 16).map_err(|_| invalid("invalid numeric bits"))?,
        );
    }
    VerifiedCenter::restore_checked(
        Vec2::new(parsed[0], parsed[1]),
        Vec2::new(parsed[2], parsed[3]),
        parsed[4],
    )
    .ok_or_else(|| invalid("profile calibration quality invalid"))
}
pub fn load(path: &Path, identity: &Identity) -> io::Result<VerifiedCenter> {
    let f = fs::File::open(path)?;
    if f.metadata()?.len() > MAX_BYTES {
        return Err(invalid("profile too large"));
    }
    let mut bytes = Vec::with_capacity(MAX_BYTES as usize);
    f.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    parse(&bytes, identity)
}
/// Save through a create_new temporary file and OS atomic replacement.
/// File writing and profile parsing are intentionally OFF the realtime path.
pub fn save(path: &Path, identity: &Identity, verified: &VerifiedCenter) -> io::Result<()> {
    let bytes = serialize(identity, verified);
    let _ = parse(bytes.as_bytes(), identity)?; // refuse invalid profiles before touching disk
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut last = None;
    for _ in 0..32 {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing filename"))?
            .to_string_lossy();
        let temp = parent.join(format!(".{stem}.{}.{}.tmp", std::process::id(), id));
        let result = (|| -> io::Result<()> {
            let mut f = match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
                Err(e) => return Err(e),
            };
            f.write_all(bytes.as_bytes())?;
            f.sync_all()?;
            drop(f);
            // Avoid rewriting a mismatched/corrupt profile without an explicit
            // repair flow. A recognized prior version may be safely replaced.
            if path.exists() {
                let _existing = load(path, identity)?;
                replace_existing(path, &temp)?;
            } else {
                fs::rename(&temp, path)?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        match result {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            other => return other,
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("cannot allocate unique temporary profile")))
}
#[cfg(windows)]
fn replace_existing(path: &Path, temp: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    #[link(name = "Kernel32")]
    extern "system" {
        fn ReplaceFileW(
            lpReplacedFileName: *const u16,
            lpReplacementFileName: *const u16,
            lpBackupFileName: *const u16,
            dwReplaceFlags: u32,
            lpExclude: *const std::ffi::c_void,
            lpReserved: *const std::ffi::c_void,
        ) -> i32;
    }
    let mut a: Vec<u16> = path.as_os_str().encode_wide().collect();
    a.push(0);
    let mut b: Vec<u16> = temp.as_os_str().encode_wide().collect();
    b.push(0);
    // REPLACEFILE_IGNORE_MERGE_ERRORS = 0x2. No backup requested.
    let ok = unsafe {
        ReplaceFileW(
            a.as_ptr(),
            b.as_ptr(),
            ptr::null(),
            2,
            ptr::null(),
            ptr::null(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn replace_existing(path: &Path, temp: &Path) -> io::Result<()> {
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        guided::GuidedWizard,
        motion_first::{MotionFirst, MotionFirstConfig},
        InputMode,
    };
    fn verified(center: Vec2) -> VerifiedCenter {
        let mut w = GuidedWizard::new();
        w.confirm_release().unwrap();
        for _ in 0..128 {
            w.sample(center).unwrap();
        }
        let shifted = if center.x > 0.5 {
            center + Vec2::new(-0.12, 0.0)
        } else {
            center + Vec2::new(0.12, 0.0)
        };
        w.sample(shifted.clamp()).unwrap();
        w.confirm_movement_finished().unwrap();
        w.confirm_release().unwrap();
        for _ in 0..128 {
            w.sample(center).unwrap();
        }
        w.commit().unwrap()
    }
    fn path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "guardadaptive-profile-{}-{}.bin",
            name,
            std::process::id()
        ))
    }
    #[test]
    fn exact_roundtrip_restores_same_filter_center() {
        let p = path("roundtrip");
        let identity = Identity::new("Controller-001", Stick::Left, Resolution::XInput16).unwrap();
        let v = verified(Vec2::new(0.55, -0.2));
        let _ = fs::remove_file(&p);
        save(&p, &identity, &v).unwrap();
        let reloaded = load(&p, &identity).unwrap();
        assert_eq!(reloaded.center(), v.center());
        let mut g = MotionFirst::new(MotionFirstConfig {
            input_lsb: 0.0,
            softness_lsb: 0.0,
        });
        for _ in 0..400 {
            g.process(Vec2::ZERO, InputMode::CalibrateNeutral);
        }
        g.accept_reacquisition(reloaded);
        assert!(g.process(v.center(), InputMode::Live).norm() < 1e-12);
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn different_stick_or_device_cannot_import_profile() {
        let p = path("identity");
        let _ = fs::remove_file(&p);
        let left = Identity::new("Controller-001", Stick::Left, Resolution::XInput16).unwrap();
        save(&p, &left, &verified(Vec2::new(0.2, 0.0))).unwrap();
        let right = Identity::new("Controller-001", Stick::Right, Resolution::XInput16).unwrap();
        let other = Identity::new("Controller-002", Stick::Left, Resolution::XInput16).unwrap();
        assert!(load(&p, &right).is_err());
        assert!(load(&p, &other).is_err());
        assert_eq!(load(&p, &left).unwrap().center().x, 0.2);
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn corruption_and_torn_write_are_rejected() {
        let p = path("torn");
        let _ = fs::remove_file(&p);
        let id = Identity::new("Controller-001", Stick::Left, Resolution::XInput16).unwrap();
        save(&p, &id, &verified(Vec2::new(0.2, 0.0))).unwrap();
        let original = fs::read(&p).unwrap();
        let mut bad = original.clone();
        bad[14] ^= 1;
        fs::write(&p, bad).unwrap();
        assert!(load(&p, &id).is_err());
        assert!(save(&p, &id, &verified(Vec2::new(0.3, 0.0))).is_err());
        assert_ne!(fs::read(&p).unwrap(), original);
        fs::write(&p, &original[..original.len() / 2]).unwrap();
        assert!(load(&p, &id).is_err());
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn replacing_a_valid_profile_is_atomic_and_loadable() {
        let p = path("replace");
        let _ = fs::remove_file(&p);
        let id = Identity::new("Controller-001", Stick::Right, Resolution::Signed12).unwrap();
        save(&p, &id, &verified(Vec2::new(0.1, -0.2))).unwrap();
        save(&p, &id, &verified(Vec2::new(0.4, -0.2))).unwrap();
        assert_eq!(load(&p, &id).unwrap().center(), Vec2::new(0.4, -0.2));
        let _ = fs::remove_file(&p);
    }
    #[test]
    fn id_cannot_contain_path_separators_or_delimiters() {
        for id in ["C:/path", "foo|left", "   ", "../bad", "", "a"] {
            assert!(Identity::new(id, Stick::Left, Resolution::Ideal).is_err());
        }
    }
}
