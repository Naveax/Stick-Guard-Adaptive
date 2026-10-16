//! Single-binary local-browser UI. Authenticated operator actions are loopback
//! only; all axis acquisition is READ ONLY. No drive, HID or XInput write APIs.
use crate::{
    demo::{DemoEngine, Scenario},
    guided::{GuidedWizard, Screen},
    profile_store::{self, Identity, Stick},
    resolution::Resolution,
    safety_preview::{PreviewMode, SafePreview},
    ui_neutral::{Session as NeutralSession, Verdict as NeutralVerdict},
    xinput::{self, Report},
    Vec2,
};
use std::{
    env,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::fs::File;
const PAGE: &str = include_str!("ui_page.html");
struct UiState {
    slot: u32,
    stick: Stick,
    alias: String,
    report: Option<Report>,
    demo: Option<DemoEngine>,
    wizard: Option<GuidedWizard>,
    preview: Option<SafePreview>,
    corrected: Option<Vec2>,
    saved: bool,
    neutral: Option<NeutralSession>,
    neutral_center: Option<Vec2>,
    neutral_result: String,
    connection_session: u64,
    message: String,
}
impl UiState {
    fn new() -> Self {
        Self {
            slot: 0,
            stick: Stick::Left,
            alias: "MY-CONTROLLER-01".to_owned(),
            report: None,
            demo: None,
            wizard: None,
            preview: None,
            corrected: None,
            saved: false,
            neutral: None,
            neutral_center: None,
            neutral_result: "not_started".into(),
            connection_session: 0,
            message: "Kontrolcu bagli degilse kalibrasyon baslatilamaz.".to_owned(),
        }
    }
    fn stage(&self) -> &'static str {
        if self.demo.is_some() {
            return "Demo";
        }
        match &self.wizard {
            Some(w) => match w.screen() {
                Screen::AskFirstRelease => "AskFirstRelease",
                Screen::SampleFirst => "SampleFirst",
                Screen::AskMovement => "AskMovement",
                Screen::AskSecondRelease => "AskSecondRelease",
                Screen::SampleSecond => "SampleSecond",
                Screen::Review => "Review",
                Screen::Cancelled => "Cancelled",
            },
            None => {
                if self.saved {
                    "Saved"
                } else {
                    "Idle"
                }
            }
        }
    }
    fn instruction(&self) -> &'static str {
        if self.demo.is_some() {
            return "SENTETIK DEMO. Gercek kontrolcu degil; bu modda profil kaydedilemez.";
        }
        self.wizard
            .as_ref()
            .map(GuidedWizard::instruction)
            .unwrap_or(if self.saved {
                "Profil kaydedildi. Secilen analogun duzeltilmis verisi yalnizca burada onizlenir."
            } else {
                "Kontrolcuyu bagla ve kalibrasyon baslat. Analog hareketini gozlemle."
            })
    }
    fn remaining(&self) -> usize {
        self.wizard
            .as_ref()
            .map_or(0, GuidedWizard::samples_remaining)
    }
    fn sample_report(&mut self, report: Option<Report>) {
        // The demo has a separate synthetic generator and never enters
        // the user-consented calibration path or stored controller profile.
        if let Some(demo) = self.demo.as_mut() {
            demo.step();
        }
        if let Some(raw) = report {
            self.report = Some(raw);
            if let Some(session) = self.neutral.as_mut() {
                let verdict =
                    session.sample(raw, self.slot, self.connection_session, Instant::now());
                self.neutral_result = verdict.name().into();
                self.corrected = None;
                match verdict {
                    NeutralVerdict::Collecting => {}
                    NeutralVerdict::Stable => {
                        self.neutral = None;
                        self.message="Operator onayli notr denetim tutarli; eski merkez degistirilmedi, sadece onizleme devam edebilir.".into();
                    }
                    _ => {
                        self.neutral = None;
                        self.preview = None;
                        self.neutral_center = None;
                        self.saved = false;
                        self.message=format!("Notr denetim sonucu: {}. Onizleme kapatildi; yeniden kalibrasyon gerekiyor.",verdict.name());
                    }
                }
                return;
            }
            if let Some(engine) = &mut self.preview {
                self.corrected = Some(engine.process(raw.stick(self.stick)));
            }
            if let Some(w) = &mut self.wizard {
                if matches!(
                    w.screen(),
                    Screen::SampleFirst | Screen::SampleSecond | Screen::AskMovement
                ) {
                    if let Err(e) = w.sample(raw.stick(self.stick)) {
                        self.message = format!("Kalibrasyon reddedildi: {e:?}");
                    }
                }
            }
        } else {
            // Slot identity is not a stable physical identity. Disconnect
            // invalidates EVERY prior preview/calibration authorization.
            if self.report.is_some() || self.neutral.is_some() {
                self.neutral_result = "controller_disconnected_audit_cancelled".into();
                self.neutral = None;
                self.neutral_center = None;
                self.connection_session = self.connection_session.wrapping_add(1);
                self.preview = None;
                self.saved = false;
                self.message =
                    "Kontrolcu ayrildi: eski kalibrasyon pasif. Yeniden kalibre et.".into();
            }
            if let Some(w) = &mut self.wizard {
                w.cancel();
                self.message = "Kontrolcu baglantisi kesildi. Kalibrasyon iptal edildi.".into();
            }
            self.report = None;
            self.corrected = None;
        }
    }
    fn status(&self) -> String {
        fn metric(v: f64) -> String {
            format!("{v:.8}")
        }
        fn pair(v: Option<Vec2>) -> String {
            v.map_or("null".to_owned(), |v| format!("[{:.9},{:.9}]", v.x, v.y))
        }
        let snap = self.demo.as_ref().and_then(DemoEngine::latest);
        let left = snap.map(|f| f.left).or_else(|| self.report.map(|r| r.left));
        let right = snap
            .map(|f| f.right)
            .or_else(|| self.report.map(|r| r.right));
        let raw = snap
            .map(|f| f.left)
            .or_else(|| self.report.map(|r| r.stick(self.stick)));
        let corrected = snap.map(|f| f.corrected).or(self.corrected);
        let demo_scenario = self.demo.as_ref().map_or("none", |d| d.scenario().name());
        let quality = self.preview.as_ref().map(SafePreview::quality);
        let demo_extra = format!(
            r#","source":"{}","demo_scenario":"{}","demo_truth":{},"demo_samples":{},"demo_raw_rmse":{},"demo_corrected_rmse":{},"demo_micro_lost":{},"demo_reversed":{},"demo_noise_rmse":{},"demo_noise_micro_lost":{},"demo_noise_reversed":{},"demo_noise_output":{},"demo_hybrid_rmse":{},"demo_hybrid_micro_lost":{},"demo_hybrid_reversed":{},"demo_hybrid_output":{},"demo_hybrid_noise_bypass":{},"demo_hybrid_calibration_noise":{},"demo_direction_rmse":{},"demo_direction_micro_lost":{},"demo_direction_reversed":{},"demo_direction_blended":{},"demo_direction_output":{},"demo_direction_max_radius_delta":{},"demo_confidence_rmse":{},"demo_confidence_micro_lost":{},"demo_confidence_reversed":{},"demo_confidence_rotations":{},"demo_confidence_output":{}"#,
            if self.demo.is_some() { "DEMO" } else { "REAL" },
            demo_scenario,
            pair(snap.map(|f| f.desired)),
            snap.map_or(0, |f| f.count),
            metric(snap.map_or(0.0, |f| f.raw_rmse)),
            metric(snap.map_or(0.0, |f| f.corrected_rmse)),
            snap.map_or(0, |f| f.micro_lost),
            snap.map_or(0, |f| f.reversed),
            metric(snap.map_or(0.0, |f| f.noise_corrected_rmse)),
            snap.map_or(0, |f| f.noise_micro_lost),
            snap.map_or(0, |f| f.noise_reversed),
            pair(snap.map(|f| f.noise_corrected)),
            metric(snap.map_or(0.0, |f| f.hybrid_corrected_rmse)),
            snap.map_or(0, |f| f.hybrid_micro_lost),
            snap.map_or(0, |f| f.hybrid_reversed),
            pair(snap.map(|f| f.hybrid_corrected)),
            snap.map_or(0, |f| f.hybrid_noise_bypass),
            metric(snap.map_or(0.0, |f| f.hybrid_calibrated_noise)),
            metric(snap.map_or(0.0, |f| f.direction_corrected_rmse)),
            snap.map_or(0, |f| f.direction_micro_lost),
            snap.map_or(0, |f| f.direction_reversed),
            snap.map_or(0, |f| f.direction_blended),
            pair(snap.map(|f| f.direction_corrected)),
            metric(snap.map_or(0.0, |f| f.direction_max_radius_delta)),
            metric(snap.map_or(0.0, |f| f.confidence_corrected_rmse)),
            snap.map_or(0, |f| f.confidence_micro_lost),
            snap.map_or(0, |f| f.confidence_reversed),
            snap.map_or(0, |f| f.confidence_rotations),
            pair(snap.map(|f| f.confidence_corrected))
        );
        format!(
            r#"{{"connected":{},"slot":{},"left":{},"right":{},"selected_raw":{},"corrected":{},"profile":{},"stage":"{}","instruction":"{}","remaining":{},"message":"{}","preview_mode":"{}","calibration_mad":{},"calibration_agreement":{},"calibration_uncertainty_bound":{},"virtual_gamepad_backend":"unavailable","game_input_writes_enabled":false,"automatic_candidate_promotion":false,"neutral_audit_active":{},"neutral_audit_remaining":{},"neutral_audit_result":"{}","neutral_release_physically_verified":false{} }}"#,
            self.report.is_some(),
            self.slot,
            pair(left),
            pair(right),
            pair(raw),
            pair(corrected),
            self.preview.is_some() && self.demo.is_none(),
            self.stage(),
            json_escape(self.instruction()),
            self.remaining(),
            json_escape(&self.message),
            quality.map_or("none", |q| q.mode.name()),
            metric(quality.map_or(0.0, |q| q.max_axis_mad)),
            metric(quality.map_or(0.0, |q| q.release_disagreement)),
            metric(quality.map_or(0.0, |q| q.uncertainty_bound)),
            self.neutral.is_some(),
            self.neutral.as_ref().map_or(0, NeutralSession::remaining),
            json_escape(&self.neutral_result),
            demo_extra
        )
    }
    fn action(&mut self, fields: &[(String, String)], profile_root: &Path) -> io::Result<()> {
        let get = |name: &str| {
            fields
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        let action = get("action").ok_or_else(|| invalid("missing action"))?;
        if self.neutral.is_some() && !matches!(action, "neutral_cancel" | "preview_disable") {
            return Err(invalid(
                "neutral audit active; finish or cancel before other actions",
            ));
        }
        match action {
            "neutral_start" => {
                if get("released") != Some("confirmed") {
                    return Err(invalid("explicit released-stick confirmation required"));
                }
                if self.demo.is_some() || self.wizard.is_some() || self.report.is_none() {
                    return Err(invalid(
                        "real controller and completed calibration required",
                    ));
                }
                let center = self
                    .neutral_center
                    .ok_or_else(|| invalid("no center in this session"))?;
                let quality = self
                    .preview
                    .as_ref()
                    .map(SafePreview::quality)
                    .ok_or_else(|| invalid("no active profile preview"))?;
                if quality.mode != PreviewMode::Calibrated {
                    return Err(invalid("uncertain calibration cannot start audit"));
                }
                let session = NeutralSession::new(
                    center,
                    self.slot,
                    self.stick,
                    self.connection_session,
                    true,
                )
                .ok_or_else(|| invalid("invalid audit parameters"))?;
                self.neutral = Some(session);
                self.corrected = None;
                self.neutral_result = "collecting_operator_declared_neutral".into();
                self.message="Kullanici analogu biraktigini ONAYLADI. 128 XInput okumasinda yalnizca ham veri; otomatik merkez guncellemesi YOK.".into();
                Ok(())
            }
            "neutral_cancel" => {
                if self.neutral.is_none() {
                    return Err(invalid("no active neutral audit"));
                }
                self.neutral = None;
                self.preview = None;
                self.neutral_center = None;
                self.saved = false;
                self.corrected = None;
                self.neutral_result = "cancelled_preview_revoked".into();
                self.message="Notr denetim kullanici tarafindan iptal edildi; onizleme yeniden kalibrasyona kadar kapali.".into();
                Ok(())
            }
            "preview_disable" => {
                if self.demo.is_some()
                    || self
                        .wizard
                        .as_ref()
                        .is_some_and(|w| w.screen() != Screen::Cancelled)
                {
                    return Err(invalid("stop demo or calibration before disabling preview"));
                }
                self.preview = None;
                self.corrected = None;
                self.saved = false;
                self.neutral = None;
                self.neutral_center = None;
                self.neutral_result = "preview_disabled".into();
                self.message="Onizleme kapatildi. Kayitli profil silinmedi; devam icin yeni kullanici kalibrasyonu gerekir.".into();
                Ok(())
            }
            "profile_inventory" => {
                if self.demo.is_some()
                    || self
                        .wizard
                        .as_ref()
                        .is_some_and(|w| w.screen() != Screen::Cancelled)
                {
                    return Err(invalid("inventory requires idle state"));
                }
                let inv = crate::profile_inventory::scan(profile_root)?;
                let names = inv
                    .rows
                    .iter()
                    .take(8)
                    .map(|row| {
                        format!(
                            "{}:{}={}",
                            row.alias,
                            row.stick.name(),
                            if row.valid { "DOSYA_OK" } else { "REDDEDILDI" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                // Auditing must not change preview, saved flags or slot.
                // File metadata is NOT an authenticated hardware binding.
                self.message=format!("Profil envanteri: {} gecerli, {} bozuk, {} ilgisiz. {}. Cihaz kimligi dogrulanmadi; hicbir profil etkinlestirilmedi.",
                    inv.valid,inv.invalid,inv.ignored,names);
                Ok(())
            }
            "profile_audit" => {
                if self.demo.is_some()
                    || self
                        .wizard
                        .as_ref()
                        .is_some_and(|w| w.screen() != Screen::Cancelled)
                {
                    return Err(invalid("profile audit requires idle state"));
                }
                let slot = get("slot")
                    .ok_or_else(|| invalid("missing slot"))?
                    .parse::<u32>()
                    .map_err(|_| invalid("bad slot"))?;
                if slot > 3 {
                    return Err(invalid("slot must be 0..3"));
                }
                let stick = match get("stick") {
                    Some("left") => Stick::Left,
                    Some("right") => Stick::Right,
                    _ => return Err(invalid("invalid stick")),
                };
                let alias = get("alias").ok_or_else(|| invalid("missing alias"))?;
                let id = Identity::new(alias, stick, Resolution::XInput16)?;
                let file = profile_path(profile_root, &id);
                let verified = profile_store::load(&file, &id)?;
                let quality = SafePreview::from_verified(verified).quality();
                // Crucial: reading a profile does NOT authorize using it.
                // XInput slot/alias cannot authenticate a physical device.
                self.message=format!("Profil dosyasi denetlendi: {} | mode={} | MAD={:.6} | Cihaz kimligi DOGRULANMADI, profil ETKINLESTIRILMEDI.",
                    file.display(),quality.mode.name(),quality.max_axis_mad);
                Ok(())
            }
            "demo_start" => {
                if self
                    .wizard
                    .as_ref()
                    .is_some_and(|w| w.screen() != Screen::Cancelled)
                {
                    return Err(invalid("finish or cancel real calibration first"));
                }
                let scenario = Scenario::parse(
                    get("scenario").ok_or_else(|| invalid("missing demo scenario"))?,
                )
                .ok_or_else(|| invalid("unknown synthetic scenario"))?;
                self.wizard = None; // discarded cancelled wizard cannot return as an active calibration
                self.demo = Some(DemoEngine::new(scenario));
                // Run an initial sample before the next scheduled device poll.
                if let Some(demo) = self.demo.as_mut() {
                    demo.step();
                }
                self.message = format!(
                    "SENTETIK DEMO: {} | hicbir profil yazilmadi",
                    scenario.name()
                );
                Ok(())
            }
            "demo_stop" => {
                if self.demo.is_none() {
                    return Err(invalid("demo is not running"));
                }
                self.demo = None;
                self.corrected = None;
                self.message = "Sentetik demo durduruldu. Gercek cihaz moduna donuldu.".into();
                Ok(())
            }
            "cancel" => {
                if self.demo.is_some() {
                    return Err(invalid("synthetic demo must use demo_stop"));
                }
                if let Some(w) = &mut self.wizard {
                    w.cancel();
                }
                self.wizard = None;
                self.message = "Kalibrasyon iptal edildi. Eski profil degismedi.".into();
                Ok(())
            }
            "select" | "start" => {
                if self.demo.is_some() {
                    return Err(invalid("stop synthetic demo first"));
                }
                if self
                    .wizard
                    .as_ref()
                    .is_some_and(|w| w.screen() != Screen::Cancelled)
                {
                    return Err(invalid("calibration active; cancel before changing device"));
                }
                self.wizard = None;
                let slot = get("slot")
                    .ok_or_else(|| invalid("missing slot"))?
                    .parse::<u32>()
                    .map_err(|_| invalid("invalid slot"))?;
                if slot > 3 {
                    return Err(invalid("XInput slot must be 0..3"));
                }
                let stick = match get("stick") {
                    Some("left") => Stick::Left,
                    Some("right") => Stick::Right,
                    _ => return Err(invalid("invalid stick")),
                };
                let alias = get("alias").ok_or_else(|| invalid("missing alias"))?;
                let _ = Identity::new(alias, stick, Resolution::XInput16)?;
                if action == "start" && xinput::read_slot(slot)?.is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "No controller detected",
                    ));
                }
                let changed = self.slot != slot || self.stick != stick || self.alias != alias;
                self.slot = slot;
                self.stick = stick;
                self.alias = alias.to_owned();
                self.report = None;
                if changed {
                    self.preview = None;
                    self.corrected = None;
                    self.saved = false;
                    self.neutral_center = None;
                    self.connection_session = self.connection_session.wrapping_add(1);
                }
                if action == "start" {
                    // A fresh calibration explicitly invalidates the old
                    // preview even if slot/stick/alias are unchanged.
                    self.preview = None;
                    self.corrected = None;
                    self.saved = false;
                    self.neutral_center = None;
                    self.wizard = Some(GuidedWizard::new());
                    self.message = "Ilk notr olcumu icin kullanici onayi gerekli.".into();
                } else {
                    self.message = "Secim degisti; aktif profil temizlendi.".into();
                }
                Ok(())
            }
            "confirm" => {
                if self.demo.is_some() {
                    return Err(invalid("synthetic demo cannot confirm or save profiles"));
                }
                let w = self
                    .wizard
                    .as_mut()
                    .ok_or_else(|| invalid("no active calibration"))?;
                if self.report.is_none() {
                    return Err(invalid("controller not connected"));
                }
                match w.screen() {
                    Screen::AskFirstRelease | Screen::AskSecondRelease => w
                        .confirm_release()
                        .map_err(|e| invalid(&format!("{e:?}")))?,
                    Screen::AskMovement => w
                        .confirm_movement_finished()
                        .map_err(|e| invalid(&format!("{e:?}")))?,
                    Screen::Review => {
                        // All file writes happen in response to explicit final user action.
                        // Refuse to consume the wizard if a future invalid alias could
                        // generate an invalid path.
                        let id = Identity::new(&self.alias, self.stick, Resolution::XInput16)?;
                        if xinput::read_slot(self.slot)?.is_none() {
                            return Err(io::Error::new(
                                io::ErrorKind::NotConnected,
                                "controller disconnected before final save",
                            ));
                        }
                        let path = profile_path(profile_root, &id);
                        let wizard = self.wizard.take().ok_or_else(|| invalid("no wizard"))?;
                        let verified = wizard.commit().map_err(|e| invalid(&format!("{e:?}")))?;
                        profile_store::save(&path, &id, &verified)?;
                        let guarded = SafePreview::from_verified(verified);
                        let permitted = guarded.quality().mode == PreviewMode::Calibrated;
                        self.neutral_center = if permitted {
                            Some(verified.center())
                        } else {
                            None
                        };
                        self.preview = Some(guarded);
                        self.saved = true;
                        self.message = if permitted {
                            format!(
                                "Profil kaydedildi: {}. Sadece onizleme aktif.",
                                path.display()
                            )
                        } else {
                            format!("Profil kaydedildi: {}. Kalibrasyon belirsiz; DUZELTME KAPALI, ham veri.",path.display())
                        };
                        return Ok(());
                    }
                    _ => return Err(invalid("current stage does not accept confirmation")),
                }
                self.message = "Asama onaylandi. Talimati uygula.".into();
                Ok(())
            }
            _ => Err(invalid("unsupported action")),
        }
    }
}
fn profile_path(root: &Path, id: &Identity) -> PathBuf {
    // Colon is permitted in the operator alias but forbidden in Windows
    // filenames (and can address NTFS alternate streams). Encode losslessly.
    root.join(format!(
        "{}-{}.profile",
        id.device_id.replace('%', "%25").replace(':', "%3A"),
        id.stick.name()
    ))
}
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}
fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
fn decode(s: &str) -> io::Result<String> {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() {
                return Err(invalid("invalid percent encoding"));
            }
            let h = |x: u8| match x {
                b'0'..=b'9' => Some(x - b'0'),
                b'a'..=b'f' => Some(x - b'a' + 10),
                b'A'..=b'F' => Some(x - b'A' + 10),
                _ => None,
            };
            let a = h(b[i + 1]).ok_or_else(|| invalid("invalid percent encoding"))?;
            let c = h(b[i + 2]).ok_or_else(|| invalid("invalid percent encoding"))?;
            out.push(a * 16 + c);
            i += 3;
        } else if b[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| invalid("invalid UTF8"))
}
fn form(body: &str) -> io::Result<Vec<(String, String)>> {
    if body.len() > 2048 {
        return Err(invalid("form too large"));
    }
    body.split('&')
        .map(|x| {
            let (a, b) = x
                .split_once('=')
                .ok_or_else(|| invalid("malformed form field"))?;
            Ok((decode(a)?, decode(b)?))
        })
        .collect()
}
#[cfg(windows)]
fn secure_token() -> io::Result<String> {
    use std::ffi::c_void;
    #[link(name = "Kernel32")]
    extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }
    type Rng = unsafe extern "system" fn(*mut c_void, *mut u8, u32, u32) -> i32;
    let library: Vec<u16> = "bcrypt.dll"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let module = unsafe { LoadLibraryExW(library.as_ptr(), std::ptr::null_mut(), 0x800) };
    if module.is_null() {
        return Err(io::Error::last_os_error());
    }
    let addr = unsafe { GetProcAddress(module, c"BCryptGenRandom".as_ptr().cast::<u8>()) };
    if addr.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "BCryptGenRandom missing",
        ));
    }
    let rng: Rng = unsafe { std::mem::transmute(addr) };
    let mut bytes = [0_u8; 24];
    // BCRYPT_USE_SYSTEM_PREFERRED_RNG=0x2, NTSTATUS=0 on success.
    let code = unsafe {
        rng(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            2,
        )
    };
    if code != 0 {
        return Err(io::Error::other(format!(
            "CNG RNG error 0x{:x}",
            code as u32
        )));
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}
#[cfg(not(windows))]
fn secure_token() -> io::Result<String> {
    let mut f = File::open("/dev/urandom")?;
    let mut bytes = [0_u8; 24];
    f.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}
fn reply(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) -> io::Result<()> {
    let payload = body.as_bytes();
    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; form-action 'none'\r\n\r\n",payload.len())?;
    stream.write_all(payload)?;
    stream.flush()
}
type HttpRequest = (String, String, Vec<(String, String)>, String);
fn read_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        if bytes.len() > 8192 {
            return Err(invalid("HTTP header too large"));
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(invalid("incomplete request"));
        }
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| invalid("HTTP header invalid UTF8"))?;
    let mut lines = headers.split("\r\n");
    let first = lines.next().ok_or_else(|| invalid("missing HTTP method"))?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let version = parts.next().unwrap_or("");
    if !["GET", "POST"].contains(&method.as_str()) || version != "HTTP/1.1" {
        return Err(invalid("unsupported HTTP request"));
    }
    let mut fields = Vec::new();
    let mut length = 0_usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("bad HTTP header"))?;
        let key = name.trim().to_ascii_lowercase();
        let val = value.trim().to_string();
        if key == "content-length" {
            length = val.parse().map_err(|_| invalid("bad content length"))?;
        }
        if key == "transfer-encoding" {
            return Err(invalid("transfer encoding unsupported"));
        }
        fields.push((key, val));
    }
    if length > 2048 {
        return Err(invalid("POST payload too large"));
    }
    if bytes.len() - header_end > length {
        return Err(invalid("extraneous POST body bytes"));
    }
    while bytes.len() - header_end < length {
        let take = chunk.len().min(length - (bytes.len() - header_end));
        let n = stream.read(&mut chunk[..take])?;
        if n == 0 {
            return Err(invalid("incomplete POST body"));
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    let body = std::str::from_utf8(&bytes[header_end..])
        .map_err(|_| invalid("body is not UTF8"))?
        .to_string();
    Ok((method, path, fields, body))
}
fn handle(
    mut stream: TcpStream,
    shared: &Arc<Mutex<UiState>>,
    token: &str,
    port: u16,
    root: &Path,
) -> io::Result<()> {
    let (method, path, headers, body) = match read_request(&mut stream) {
        Ok(req) => req,
        Err(e) => {
            reply(&mut stream, "400 Bad Request", "text/plain", "Bad request")?;
            return Err(e);
        }
    };
    let get = |k: &str| {
        headers
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    let allow_host = format!("127.0.0.1:{port}");
    let local_host = format!("localhost:{port}");
    if get("host") != Some(allow_host.as_str()) && get("host") != Some(local_host.as_str()) {
        reply(&mut stream, "403 Forbidden", "text/plain", "Invalid host")?;
        return Ok(());
    }
    if method == "GET" && path == "/" {
        return reply(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            &PAGE.replace("__GUARD_TOKEN__", token),
        );
    }
    if method == "GET" && path == "/api/profile-catalog" {
        // Profile aliases are local user data. Unlike public /api/status,
        // catalog rows require the session token and never mutate UI state.
        if get("x-guard-token") != Some(token) {
            return reply(
                &mut stream,
                "403 Forbidden",
                "application/json",
                r#"{"error":"Authorization failed"}"#,
            );
        }
        return match crate::profile_inventory::scan(root) {
            Ok(inv) => reply(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                &inv.json(),
            ),
            Err(_) => reply(
                &mut stream,
                "400 Bad Request",
                "application/json",
                r#"{"error":"Catalog unavailable; no profile activated"}"#,
            ),
        };
    }
    if method == "GET" && path == "/api/recovery-overview" {
        // Fixed roots only. A session token is required for local filenames
        // and marker states. No client path, shell call, or disk writes.
        if get("x-guard-token") != Some(token) {
            return reply(
                &mut stream,
                "403 Forbidden",
                "application/json",
                r#"{"error":"Authorization failed"}"#,
            );
        }
        let overview = crate::recovery_overview_v49::inspect(root);
        return reply(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            &overview.json(),
        );
    }
    if method == "GET" && path == "/api/status" {
        let state = shared
            .lock()
            .map_err(|_| io::Error::other("state poisoned"))?;
        return reply(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            &state.status(),
        );
    }
    if method == "POST" && path == "/api/action" {
        let valid_origin = matches!(get("origin"),Some(v) if v==format!("http://127.0.0.1:{port}") || v==format!("http://localhost:{port}"));
        if get("x-guard-token") != Some(token)
            || !valid_origin
            || get("content-type") != Some("application/x-www-form-urlencoded")
        {
            return reply(
                &mut stream,
                "403 Forbidden",
                "application/json",
                r#"{"error":"Authorization failed"}"#,
            );
        }
        let params = match form(&body) {
            Ok(v) => v,
            Err(e) => {
                return reply(
                    &mut stream,
                    "400 Bad Request",
                    "application/json",
                    &format!(r#"{{"error":"{}"}}"#, json_escape(&e.to_string())),
                )
            }
        };
        let mut state = shared
            .lock()
            .map_err(|_| io::Error::other("state poisoned"))?;
        return match state.action(&params, root) {
            Ok(()) => reply(&mut stream, "200 OK", "application/json", r#"{"ok":true}"#),
            Err(e) => {
                state.message = e.to_string();
                reply(
                    &mut stream,
                    "400 Bad Request",
                    "application/json",
                    &format!(r#"{{"error":"{}"}}"#, json_escape(&e.to_string())),
                )
            }
        };
    }
    reply(&mut stream, "404 Not Found", "text/plain", "Not found")
}
pub fn run(port: u16) -> io::Result<()> {
    if port < 1024 {
        return Err(invalid("port must be >=1024"));
    }
    let token = secure_token()?;
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("GuardAdaptive"));
    let profiles = base.join("GuardAdaptive").join("profiles");
    let socket = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let listener = TcpListener::bind(socket)?;
    let state = Arc::new(Mutex::new(UiState::new()));
    let polling = Arc::clone(&state);
    thread::spawn(move || loop {
        let slot = polling.lock().map(|g| g.slot).unwrap_or(0);
        let report = xinput::read_slot(slot).ok().flatten();
        if let Ok(mut s) = polling.lock() {
            if s.slot == slot {
                s.sample_report(report);
            }
        }
        thread::sleep(Duration::from_millis(8));
    });
    println!("GuardAdaptive v0.49 interactive read-only research dashboard");
    println!("Open: http://127.0.0.1:{port}/");
    println!("Local browser only; no virtual controller output. Press Ctrl+C to stop.");
    println!(
        "Profiles saved only after explicit operator confirmation: {}",
        profiles.display()
    );
    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                let _ = handle(s, &state, &token, port, &profiles);
            }
            Err(e) => eprintln!("HTTP accept error: {e}"),
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_escape_handles_untrusted_text() {
        assert_eq!(json_escape("a\"\\\n"), "a\\\"\\\\\\n");
    }
    #[test]
    fn secure_csrf_token_has_expected_entropy_length() {
        let a = secure_token().unwrap();
        let b = secure_token().unwrap();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    #[test]
    fn ui_neutral_audit_needs_explicit_release_and_does_not_recenter() {
        let mut s = UiState::new();
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.24, -0.18),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        s.preview = Some(SafePreview::from_verified(v));
        s.neutral_center = Some(v.center());
        s.saved = true;
        let neutral = Report {
            packet: 1,
            left: Vec2::new(0.24, -0.18),
            right: Vec2::ZERO,
            buttons: 9,
            left_trigger: 7,
            right_trigger: 255,
        };
        s.sample_report(Some(neutral));
        let root = Path::new("unwritten");
        assert!(s
            .action(&form("action=neutral_start").unwrap(), root)
            .is_err());
        assert!(s.neutral.is_none());
        s.action(
            &form("action=neutral_start&released=confirmed").unwrap(),
            root,
        )
        .unwrap();
        assert!(s.neutral.is_some());
        assert!(s.status().contains(r#""neutral_audit_active":true"#));
        for _ in 0..128 {
            s.sample_report(Some(neutral));
        }
        assert!(s.neutral.is_none());
        assert!(s.preview.is_some());
        assert_eq!(s.neutral_center, Some(v.center()));
        assert_eq!(s.neutral_result, "stable_declared_neutral_no_center_change");
        assert!(s
            .status()
            .contains(r#""neutral_release_physically_verified":false"#));
    }
    #[test]
    fn ui_neutral_shift_and_disconnect_revoke_preview() {
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.24, -0.18),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        let report = |x: f64| Report {
            packet: 10,
            left: Vec2::new(x, -0.18),
            right: Vec2::ZERO,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        };
        let mut s = UiState::new();
        s.preview = Some(SafePreview::from_verified(v));
        s.neutral_center = Some(v.center());
        s.saved = true;
        s.sample_report(Some(report(0.24)));
        s.action(
            &form("action=neutral_start&released=confirmed").unwrap(),
            Path::new("unused"),
        )
        .unwrap();
        for _ in 0..128 {
            s.sample_report(Some(report(0.36)));
        }
        assert_eq!(
            s.neutral_result,
            "declared_neutral_center_disagreement_recalibrate"
        );
        assert!(s.preview.is_none());
        assert!(!s.saved);
        let mut s = UiState::new();
        s.preview = Some(SafePreview::from_verified(v));
        s.neutral_center = Some(v.center());
        s.saved = true;
        s.sample_report(Some(report(0.24)));
        s.action(
            &form("action=neutral_start&released=confirmed").unwrap(),
            Path::new("unused"),
        )
        .unwrap();
        s.sample_report(None);
        assert!(s.neutral.is_none());
        assert!(s.preview.is_none());
        assert!(s.neutral_center.is_none());
        assert!(!s.saved);
    }
    #[test]
    fn ui_neutral_cancel_and_disable_are_fail_closed() {
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.24, -0.18),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        let mut s = UiState::new();
        s.preview = Some(SafePreview::from_verified(v));
        s.neutral_center = Some(v.center());
        s.saved = true;
        s.sample_report(Some(Report {
            packet: 1,
            left: Vec2::new(0.24, -0.18),
            right: Vec2::ZERO,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        }));
        s.action(
            &form("action=neutral_start&released=confirmed").unwrap(),
            Path::new("unused"),
        )
        .unwrap();
        assert!(s
            .action(
                &form("action=select&slot=1&stick=right&alias=NEW").unwrap(),
                Path::new("unused")
            )
            .is_err());
        s.action(&form("action=neutral_cancel").unwrap(), Path::new("unused"))
            .unwrap();
        assert!(s.preview.is_none());
        assert!(s.neutral_center.is_none());
        assert_eq!(s.neutral_result, "cancelled_preview_revoked");
    }
    #[test]
    fn nonconnected_ui_cannot_start_a_wizard() {
        let mut s = UiState::new();
        let args = form("action=start&slot=0&stick=left&alias=MY-CONTROLLER-01").unwrap();
        assert_eq!(
            s.action(&args, Path::new("dummy")).unwrap_err().kind(),
            io::ErrorKind::NotConnected
        );
        assert_eq!(s.stage(), "Idle");
    }
    #[test]
    fn malformed_actions_do_not_change_active_state() {
        let mut s = UiState::new();
        let invalid = form("action=start&slot=4&stick=left&alias=MY-CONTROLLER-01").unwrap();
        assert!(s.action(&invalid, Path::new("dummy")).is_err());
        assert_eq!(s.slot, 0);
        assert!(s.wizard.is_none());
    }
    #[test]
    fn percent_decode_roundtrip_and_invalid_input() {
        assert_eq!(decode("A%20B%2BC").unwrap(), "A B+C");
        assert!(decode("%GG").is_err());
        assert!(decode("%00%FF").is_err());
    }
    #[test]
    fn synthetic_ui_state_requires_explicit_commit_and_real_hardware() {
        use std::fs;
        let mut s = UiState::new();
        let folder =
            std::env::temp_dir().join(format!("guardadaptive-ui-flow-{}", std::process::id()));
        let id = Identity::new(&s.alias, s.stick, Resolution::XInput16).unwrap();
        let saved = profile_path(&folder, &id);
        let _ = fs::remove_file(&saved);
        // Only the unit test injects a synthetic report. A deployed server
        // requires an actual XInput device at the selected slot.
        s.wizard = Some(GuidedWizard::new());
        let ok = |s: &mut UiState| s.action(&form("action=confirm").unwrap(), &folder).unwrap();
        let mock = |x: f64| Report {
            packet: 1,
            left: Vec2::new(x, -0.2),
            right: Vec2::ZERO,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        };
        s.report = Some(mock(0.55));
        ok(&mut s);
        for _ in 0..128 {
            s.sample_report(Some(mock(0.55)));
        }
        assert_eq!(s.stage(), "AskMovement");
        s.sample_report(Some(mock(0.3)));
        ok(&mut s);
        assert_eq!(s.stage(), "AskSecondRelease");
        s.report = Some(mock(0.55));
        ok(&mut s);
        for _ in 0..128 {
            s.sample_report(Some(mock(0.55)));
        }
        assert_eq!(s.stage(), "Review");
        assert!(!saved.exists());
        // A disconnect before final confirmation must never write a profile,
        // even if a legitimate physical controller happens to be available.
        s.sample_report(None);
        let err = s
            .action(&form("action=confirm").unwrap(), &folder)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!saved.exists());
        let _ = fs::remove_dir(&folder);
    }
    #[test]
    fn disconnected_slot_cannot_reuse_previous_controller_calibration() {
        let mut state = UiState::new();
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.55, -0.2),
            Vec2::ZERO,
            0.0,
        )
        .unwrap();
        state.preview = Some(SafePreview::from_verified(v));
        state.saved = true;
        let frame = Report {
            packet: 1,
            left: Vec2::new(0.553, -0.2),
            right: Vec2::ZERO,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        };
        state.sample_report(Some(frame));
        assert!(state.corrected.unwrap().x < 0.01);
        assert!(state.preview.is_some());
        state.sample_report(None);
        assert!(
            state.preview.is_none(),
            "disconnect must invalidate calibration"
        );
        assert!(!state.saved);
        assert!(state.corrected.is_none());
        assert!(state.status().contains(r#""preview_mode":"none""#));
        // A different physical controller can occupy the SAME XInput slot.
        state.sample_report(Some(frame));
        assert!(state.corrected.is_none());
        assert!(state.preview.is_none());
        assert!(!state.saved);
    }
    #[test]
    fn uncertain_profile_reports_raw_mode_in_status() {
        let mut state = UiState::new();
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.55, -0.2),
            Vec2::new(0.005, 0.003),
            0.001,
        )
        .unwrap();
        state.preview = Some(SafePreview::from_verified(v));
        state.saved = true;
        state.sample_report(Some(Report {
            packet: 1,
            left: Vec2::new(0.552, -0.2),
            right: Vec2::ZERO,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        }));
        assert_eq!(state.corrected, Some(Vec2::new(0.552, -0.2)));
        let json = state.status();
        assert!(json.contains(r#""preview_mode":"uncertain_raw_passthrough""#));
        assert!(json.contains(r#""calibration_uncertainty_bound":0.01100000"#));
    }
    #[test]
    fn synthetic_demo_is_isolated_from_real_calibration_and_storage() {
        let mut s = UiState::new();
        let tmp =
            std::env::temp_dir().join(format!("guardadaptive-ui-demo-{}", std::process::id()));
        let args = form("action=demo_start&scenario=flipping").unwrap();
        s.action(&args, &tmp).unwrap();
        assert_eq!(s.stage(), "Demo");
        assert!(s.preview.is_none());
        assert!(!s.saved);
        let forbidden = form("action=confirm").unwrap();
        assert!(s.action(&forbidden, &tmp).is_err());
        let start = form("action=start&slot=0&stick=left&alias=MY-CONTROLLER-01").unwrap();
        assert!(s.action(&start, &tmp).is_err());
        assert!(s.wizard.is_none());
        for _ in 0..600 {
            s.sample_report(None);
        }
        let st = s.status();
        assert!(st.contains(r#""source":"DEMO""#));
        assert!(st.contains(r#""connected":false"#));
        assert!(st.contains(r#""demo_scenario":"flipping""#));
        assert!(st.contains(r#""demo_noise_rmse":"#));
        assert!(st.contains(r#""demo_hybrid_rmse":"#));
        assert!(st.contains(r#""demo_direction_rmse":"#));
        assert!(st.contains(r#""demo_confidence_rmse":"#));
        assert!(st.contains(r#""demo_direction_max_radius_delta":"#));
        assert!(st.contains(r#""demo_hybrid_noise_bypass":"#));
        assert!(
            !tmp.exists(),
            "the synthetic dashboard never writes profile directories"
        );
        s.action(&form("action=demo_stop").unwrap(), &tmp).unwrap();
        assert_eq!(s.stage(), "Idle");
        assert!(!s.saved);
    }
    #[test]
    fn demo_start_stop_does_not_replace_existing_verified_preview() {
        let mut s = UiState::new();
        s.preview = Some(SafePreview::from_verified(
            crate::reacquire::VerifiedCenter::restore_checked(Vec2::ZERO, Vec2::ZERO, 0.0).unwrap(),
        ));
        s.saved = true;
        assert_eq!(s.stage(), "Saved");
        s.action(
            &form("action=demo_start&scenario=creeping").unwrap(),
            Path::new("unused"),
        )
        .unwrap();
        assert_eq!(s.stage(), "Demo");
        assert!(s.preview.is_some());
        assert!(s.saved);
        assert!(s.status().contains(r#""profile":false"#));
        for _ in 0..100 {
            s.sample_report(None);
        }
        s.action(&form("action=demo_stop").unwrap(), Path::new("unused"))
            .unwrap();
        assert_eq!(s.stage(), "Saved");
        assert!(s.preview.is_some());
        assert!(s.saved);
        assert!(s.status().contains(r#""profile":true"#));
    }
    #[test]
    fn invalid_scenario_cannot_start_demo() {
        let mut s = UiState::new();
        assert!(s
            .action(
                &form("action=demo_start&scenario=real").unwrap(),
                Path::new("unused")
            )
            .is_err());
        assert!(s.demo.is_none());
    }
    #[test]
    fn preview_disable_never_deletes_stored_profile_or_enables_game_output() {
        let mut s = UiState::new();
        s.report = Some(Report {
            packet: 1,
            left: Vec2::ZERO,
            right: Vec2::ZERO,
            buttons: 0,
            left_trigger: 0,
            right_trigger: 0,
        });
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.01, 0.0),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        s.preview = Some(SafePreview::from_verified(v));
        s.saved = true;
        s.action(
            &form("action=preview_disable").unwrap(),
            Path::new("unused"),
        )
        .unwrap();
        assert!(s.preview.is_none());
        assert!(!s.saved);
        assert!(s.status().contains(r#""game_input_writes_enabled":false"#));
        assert_eq!(s.stage(), "Idle");
    }
    #[test]
    fn inventory_action_is_read_only_and_does_not_enable_profile() {
        let root = std::env::temp_dir().join(format!("ga-ui-v33-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let id = Identity::new("TEST-PAD-33", Stick::Left, Resolution::XInput16).unwrap();
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.12, 0.0),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        let file = profile_path(&root, &id);
        profile_store::save(&file, &id, &v).unwrap();
        let before = std::fs::read(&file).unwrap();
        let mut state = UiState::new();
        state
            .action(&form("action=profile_inventory").unwrap(), &root)
            .unwrap();
        assert!(state.message.contains("1 gecerli"));
        assert!(state.message.contains("hicbir profil etkinlestirilmedi"));
        assert!(state.preview.is_none());
        assert!(!state.saved);
        assert_eq!(std::fs::read(&file).unwrap(), before);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn profile_audit_never_activates_existing_profile() {
        let root = std::env::temp_dir().join(format!("ga-ui-v31-audit-{}", std::process::id()));
        let id = Identity::new("TEST-PAD-31", Stick::Right, Resolution::XInput16).unwrap();
        let v = crate::reacquire::VerifiedCenter::restore_checked(
            Vec2::new(0.12, 0.0),
            Vec2::new(0.0001, 0.0001),
            0.0001,
        )
        .unwrap();
        profile_store::save(&profile_path(&root, &id), &id, &v).unwrap();
        let mut s = UiState::new();
        s.action(
            &form("action=profile_audit&slot=0&stick=right&alias=TEST-PAD-31").unwrap(),
            &root,
        )
        .unwrap();
        assert!(s.message.contains("ETKINLESTIRILMEDI"));
        assert!(s.preview.is_none());
        assert!(s
            .action(
                &form("action=profile_audit&slot=0&stick=left&alias=TEST-PAD-31").unwrap(),
                &root
            )
            .is_err());
        assert!(s.preview.is_none());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn profile_path_escaping_cannot_collapse_distinct_aliases() {
        let colon = Identity::new("Pad:01", Stick::Left, Resolution::XInput16).unwrap();
        assert!(
            Identity::new("Pad%3A01", Stick::Left, Resolution::XInput16).is_err(),
            "literal percent-encoded aliases are forbidden at profile validation"
        );
        let a = profile_path(Path::new("profiles"), &colon);
        assert!(a.ends_with("Pad%3A01-left.profile"));
    }
    #[test]
    fn profile_path_contains_only_approved_alias() {
        let id = Identity::new("My-Device:001", Stick::Right, Resolution::XInput16).unwrap();
        assert!(profile_path(Path::new("profiles"), &id).ends_with("My-Device%3A001-right.profile"));
    }
}
