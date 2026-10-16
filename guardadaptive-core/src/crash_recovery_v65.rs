//! v0.65 controller-free crash/restart acceptance.
//! A child process deliberately exits with code 65 while a dirty lease exists.
//! A separate process must NEVER auto-arm; only explicit reset can quarantine
//! the lease. These files are lab evidence, NOT a production authorization store.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    process,
};
const DIRTY: &str = "session-dirty-v65.txt";
const CLOSED: &str = "session-closed-v65.txt";
const DIRTY_BYTES: &[u8] = b"GUARDADAPTIVE_V65_OFFLINE_INFLIGHT_V1\n";
const CLOSED_BYTES: &[u8] = b"GUARDADAPTIVE_V65_OFFLINE_OPERATOR_RESET_V1\n";

fn safe_lab_dir(path: &Path) -> io::Result<()> {
    if !path.is_dir() || path.symlink_metadata()?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "lab must be a real existing directory",
        ));
    }
    Ok(())
}
fn put_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}
pub fn crash_child(dir: &Path) -> io::Result<()> {
    safe_lab_dir(dir)?;
    // The parent test creates a fresh lab folder. Never replace any file.
    put_new(&dir.join(DIRTY), DIRTY_BYTES)?;
    println!("V065_CHILD_DIRTY_ON_DISK=TRUE SIMULATED_UNCLEAN_PROCESS_EXIT=65");
    // Exit bypasses ordinary Rust stack unwinding and Drop-based cleanup.
    // This is an unclean termination simulation, NOT power-loss testing.
    process::exit(65);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    BlockedDirty,
    BlockedMissing,
    BlockedCorrupt,
    OffAfterConfirmedReset,
}
pub fn check(dir: &Path) -> io::Result<State> {
    safe_lab_dir(dir)?;
    let dirty = dir.join(DIRTY);
    let closed = dir.join(CLOSED);
    if closed.exists() {
        // Any dirty or corrupt marker remaining is contradictory; block.
        let c = fs::read(&closed)?;
        if c == CLOSED_BYTES && !dirty.exists() {
            return Ok(State::OffAfterConfirmedReset);
        }
        return Ok(State::BlockedCorrupt);
    }
    if !dirty.exists() {
        return Ok(State::BlockedMissing);
    }
    match fs::read(dirty) {
        Ok(bytes) if bytes == DIRTY_BYTES => Ok(State::BlockedDirty),
        _ => Ok(State::BlockedCorrupt),
    }
}
pub fn report(dir: &Path, out: &Path) -> io::Result<()> {
    let state = check(dir)?;
    let name = match state {
        State::BlockedDirty => "BLOCKED_DIRTY_UNCLEAN_EXIT",
        State::BlockedMissing => "BLOCKED_MISSING_LEASE",
        State::BlockedCorrupt => "BLOCKED_TAMPERED_LEASE",
        State::OffAfterConfirmedReset => "OFF_AFTER_CONFIRMED_RESET_NOT_ARMED",
    };
    let body = format!(
        "{{\"schema\":1,\"source\":\"v65_offline_separate_process_crash_recovery\",\
        \"state\":\"{}\",\"automatically_armed\":false,\
        \"in_process_memory_recovered\":false,\
        \"physical_provider_verified\":false,\
        \"operator_consent_remains_required\":true,\
        \"real_driver_writes\":0,\"game_input_writes\":0,\
        \"production_gate\":\"BLOCKED\"}}\n",
        name
    );
    crate::truth_lab::save_new(out, &body)?;
    println!("V065_RESTART_PROBE=PASS state={name} auto_arm=false production=BLOCKED");
    Ok(())
}
pub fn operator_reset(dir: &Path, acknowledge: &str) -> io::Result<()> {
    safe_lab_dir(dir)?;
    if acknowledge != "CONFIRM_OFFLINE_RESET_V65" {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "explicit offline reset confirmation required",
        ));
    }
    if check(dir)? != State::BlockedDirty {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "only intact dirty lease can be reset",
        ));
    }
    // Persist the OFF evidence first, then remove only our exact owned marker.
    // If interrupted between operations, probe fails closed as contradictory.
    put_new(&dir.join(CLOSED), CLOSED_BYTES)?;
    fs::remove_file(dir.join(DIRTY))?;
    if check(dir)? != State::OffAfterConfirmedReset {
        return Err(io::Error::other("confirmation did not reach OFF"));
    }
    println!("V065_OPERATOR_RESET=PASS state=OFF no_auto_rearm=true");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_autorearm_for_absent_corrupt_and_replayed_leases() {
        let dir = std::env::temp_dir().join(format!("ga-v65-{}-{}", process::id(), "states"));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir(&dir).unwrap();
        assert_eq!(check(&dir).unwrap(), State::BlockedMissing);
        assert!(operator_reset(&dir, "CONFIRM_OFFLINE_RESET_V65").is_err());
        put_new(&dir.join(DIRTY), b"evil").unwrap();
        assert_eq!(check(&dir).unwrap(), State::BlockedCorrupt);
        assert!(operator_reset(&dir, "CONFIRM_OFFLINE_RESET_V65").is_err());
        fs::remove_file(dir.join(DIRTY)).unwrap();
        put_new(&dir.join(DIRTY), DIRTY_BYTES).unwrap();
        assert_eq!(check(&dir).unwrap(), State::BlockedDirty);
        assert!(operator_reset(&dir, "").is_err());
        assert_eq!(check(&dir).unwrap(), State::BlockedDirty);
        operator_reset(&dir, "CONFIRM_OFFLINE_RESET_V65").unwrap();
        assert_eq!(check(&dir).unwrap(), State::OffAfterConfirmedReset);
        assert!(operator_reset(&dir, "CONFIRM_OFFLINE_RESET_V65").is_err());
        assert!(put_new(&dir.join(DIRTY), DIRTY_BYTES).is_ok());
        assert_eq!(check(&dir).unwrap(), State::BlockedCorrupt);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn evidence_is_non_overwriting() {
        let dir = std::env::temp_dir().join(format!("ga-v65-{}-{}", process::id(), "report"));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir(&dir).unwrap();
        let receipt = dir.join("receipt.json");
        report(&dir, &receipt).unwrap();
        let bytes = fs::read(&receipt).unwrap();
        assert!(report(&dir, &receipt).is_err());
        assert_eq!(fs::read(&receipt).unwrap(), bytes);
        fs::remove_dir_all(dir).unwrap();
    }
}
