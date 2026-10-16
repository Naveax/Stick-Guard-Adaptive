//! Local-console human-guided calibration from an actual XInput controller.
//! This is deliberately not unattended and cannot run without the operator
//! affirmatively confirming each stage. It never installs a virtual device.
use crate::{
    capture::{Reader, XInputReader},
    guided::{GuidedWizard, Screen},
    profile_store::{self, Identity, Stick},
    resolution::Resolution,
};
use std::{
    io::{self, Write},
    path::Path,
    thread,
    time::Duration,
};

fn affirm(expected: &str, message: &str) -> io::Result<()> {
    print!("{message} Onaylamak icin {expected} yaz ve ENTER'a bas: ");
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "local operator confirmation absent",
        ));
    }
    if line.trim() != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "operator did not confirm requested phase",
        ));
    }
    Ok(())
}
fn poll<R: Reader>(reader: &mut R, which: Stick) -> io::Result<crate::Vec2> {
    reader.read()?.map(|r| r.stick(which)).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "controller disconnected; no calibration profile was committed",
        )
    })
}
fn stage_error(e: crate::guided::WizardError) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("calibration rejected: {e:?}"),
    )
}
pub fn run(slot: u32, which: Stick, alias: &str, period_ms: u64, out: &Path) -> io::Result<()> {
    if slot > 3 || !(1..=100).contains(&period_ms) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "XInput slot 0..3, period 1..100ms",
        ));
    }
    let identity = Identity::new(alias, which, Resolution::XInput16)?;
    // Before any prompts or profile write, probe real hardware.
    let mut reader = XInputReader { slot };
    let _ = poll(&mut reader, which)?;
    println!(
        "XInput slot={} stick={}; read-only acquisition; no game input injection.",
        slot,
        which.name()
    );
    println!(
        "IMPORTANT: --device-id is an OPERATOR CHOSEN LABEL, NOT a verified physical hardware ID."
    );
    println!("Do not reuse profiles across controllers. Plugging another controller into the same slot is NOT identity verification.");
    let mut wizard = GuidedWizard::new();
    affirm("BASLAT", wizard.instruction())?;
    wizard.confirm_release().map_err(stage_error)?;
    for _ in 0..128 {
        let sample = poll(&mut reader, which)?;
        wizard.sample(sample).map_err(stage_error)?;
        thread::sleep(Duration::from_millis(period_ms));
    }
    if wizard.screen() != Screen::AskMovement {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "first neutral window incomplete",
        ));
    }
    affirm("HAREKET", wizard.instruction())?;
    // 400 samples provides time to exercise a controller; if movement does not
    // register, the transaction is rejected rather than guessed.
    for _ in 0..400 {
        let sample = poll(&mut reader, which)?;
        wizard.sample(sample).map_err(stage_error)?;
        thread::sleep(Duration::from_millis(period_ms));
    }
    wizard.confirm_movement_finished().map_err(stage_error)?;
    affirm("BIRAK", wizard.instruction())?;
    wizard.confirm_release().map_err(stage_error)?;
    for _ in 0..128 {
        let sample = poll(&mut reader, which)?;
        wizard.sample(sample).map_err(stage_error)?;
        thread::sleep(Duration::from_millis(period_ms));
    }
    if wizard.screen() != Screen::Review {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "second neutral window incomplete",
        ));
    }
    affirm("KAYDET", wizard.instruction())?;
    let verified = wizard.commit().map_err(stage_error)?;
    // Final device presence check: a disconnect immediately before commit is
    // treated as a failed, uncommitted transaction.
    let _ = poll(&mut reader, which)?;
    profile_store::save(out, &identity, &verified)?;
    println!(
        "PROFILE SAVED after explicit local confirmation: {} center=({:+.8},{:+.8})",
        out.display(),
        verified.center().x,
        verified.center().y
    );
    println!("Stored center is candidate calibration data; game output remains disabled and release status BLOCKED.");
    Ok(())
}
