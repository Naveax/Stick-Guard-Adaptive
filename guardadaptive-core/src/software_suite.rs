//! v0.29 integrated software-only acceptance: adversarial search, fixed
//! memory preview pipeline, fault injection and installation safety manifest.
//! No physical controller writes or game injections can be initiated here.
use crate::{
    adversarial::{self, Search},
    reacquire::VerifiedCenter,
    runtime_core::{Core, Frame, MemoryInspector, Reason, State, Timing},
    Vec2,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static RUN_ID: AtomicU64 = AtomicU64::new(0);
pub struct Trial {
    pub frames: u64,
    pub offline_previews: u64,
    pub raw_fallbacks: u64,
    pub fault_events: u64,
    pub disconnections: u64,
    pub emergency_stops: u64,
    pub sink_deliveries: u64,
    pub physical_device_writes: u64,
    pub nonfinite_outputs: u64,
    pub timing: Timing,
    pub trace: String,
}
impl Trial {
    pub fn json(&self) -> String {
        format!(
            concat!(
            "{{\"schema\":1,\"source\":\"deterministic_memory_only_runtime\",",
            "\"frames\":{},\"preview_frames\":{},\"raw_fallback_frames\":{},",
            "\"fault_events\":{},\"disconnects\":{},\"emergency_stops\":{},",
            "\"memory_inspector_packets\":{},\"physical_device_writes\":{},",
            "\"nonfinite_outputs\":{},\"timing\":{},",
            "\"stream_allocations_in_hot_path\":0,",
            "\"stream_allocation_claim_basis\":\"no_dynamic_allocation_operations_in_core_step\",",
            "\"driver_calls\":false,",
            "\"game_input_reached\":false,\"real_game_latency_measured\":false}}\n"),
            self.frames,
            self.offline_previews,
            self.raw_fallbacks,
            self.fault_events,
            self.disconnections,
            self.emergency_stops,
            self.sink_deliveries,
            self.physical_device_writes,
            self.nonfinite_outputs,
            self.timing.json()
        )
    }
}
pub fn stress(seed: u64, frames: usize) -> io::Result<Trial> {
    if !(10000..=1_000_000).contains(&frames) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stress frames must be 10000..1000000",
        ));
    }
    let profile = || {
        VerifiedCenter::restore_checked(Vec2::new(0.09, -0.04), Vec2::new(0.0001, 0.0001), 0.0001)
            .expect("bounded synthetic lab profile")
    };
    let mut rt = Core::default();
    let mut sink = MemoryInspector::default();
    let mut epoch = 1u64;
    let mut packet = 0u32;
    let mut stamp = 0u64;
    let mut rng = seed | 1;
    let mut result=Trial{
        frames:0,offline_previews:0,raw_fallbacks:0,
        fault_events:0,disconnections:0,emergency_stops:0,
        sink_deliveries:0,physical_device_writes:0,nonfinite_outputs:0,
        timing:Timing::default(),
        trace:String::from("index,packet,timestamp_us,raw_left_x,raw_left_y,shadow_left_x,shadow_left_y,buttons,lt,rt,reason,game_write\n"),
    };
    for i in 0..frames {
        if i.is_multiple_of(4001) {
            rt.disconnect();
            epoch += 1;
            result.disconnections += 1;
            packet = 0;
            stamp = 0;
        }
        if i % 4001 == 1 {
            // Synthetic human consent; never a real hardware claim.
            let armed = rt.arm(profile(), epoch, true);
            if !armed {
                return Err(io::Error::other("synthetic rearm failed"));
            }
        }
        if i.is_multiple_of(5501) {
            rt.emergency_off();
            result.emergency_stops += 1;
        }
        // Fault injection tests latching and raw return, not device writes.
        stamp += if i % 2053 == 2001 { 80_000 } else { 8_000 };
        if i % 3301 == 3001 {
            packet = packet.saturating_sub(5);
        } else if i.is_multiple_of(2) {
            packet = packet.saturating_add(1);
        }
        rng ^= rng >> 12;
        rng ^= rng << 25;
        rng ^= rng >> 27;
        let v = rng.wrapping_mul(0x2545F4914F6CDD1D);
        let u = ((v >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
        let current = Frame {
            monotonic_us: stamp,
            packet,
            left: Vec2::new(0.09 + u * 0.2, -0.04 + u * 0.1),
            right: Vec2::new(-u * 0.5, u * 0.3),
            buttons: (i as u16) ^ 0xA5A5,
            left_trigger: (i % 256) as u8,
            right_trigger: (255 - i % 256) as u8,
        };
        let tick = Instant::now();
        let out = rt.observe(current, epoch, &mut sink)?;
        result
            .timing
            .observe(tick.elapsed().as_nanos().min(u64::MAX as u128) as u64);
        result.frames += 1;
        if out.delivered_to_game {
            result.physical_device_writes += 1;
        }
        if !out.shadow.left.finite() || !out.shadow.right.finite() {
            result.nonfinite_outputs += 1;
        }
        if out.reason == Reason::PreviewOnly {
            result.offline_previews += 1;
        } else {
            result.raw_fallbacks += 1;
        }
        if rt.state() == State::LatchedFault
            && matches!(
                out.reason,
                Reason::PacketRollback
                    | Reason::StaleInput
                    | Reason::PollDiscontinuity
                    | Reason::BadAxis
            )
        {
            result.fault_events += 1;
        }
        if i < 256 {
            result.trace.push_str(&format!(
                "{i},{packet},{stamp},{:.9},{:.9},{:.9},{:.9},{},{},{},{},false\n",
                current.left.x,
                current.left.y,
                out.shadow.left.x,
                out.shadow.left.y,
                out.shadow.buttons,
                out.shadow.left_trigger,
                out.shadow.right_trigger,
                out.reason.name()
            ));
        }
        if out.original.right != out.shadow.right
            || out.original.buttons != out.shadow.buttons
            || out.original.left_trigger != out.shadow.left_trigger
            || out.original.right_trigger != out.shadow.right_trigger
        {
            return Err(io::Error::other("non-axis control corrupted"));
        }
    }
    result.sink_deliveries = sink.observed;
    if result.physical_device_writes > 0 || result.nonfinite_outputs > 0 {
        return Err(io::Error::other("unsafe runtime invariants"));
    }
    Ok(result)
}
pub struct Package {
    pub search: Search,
    pub runtime: Trial,
}
impl Package {
    pub fn evaluate(seed: u64, samples: usize, stress_frames: usize) -> io::Result<Self> {
        Ok(Self {
            search: adversarial::search(seed, samples)?,
            runtime: stress(seed, stress_frames)?,
        })
    }
    pub fn manifest(&self) -> String {
        let pass = self.runtime.physical_device_writes == 0
            && self.runtime.nonfinite_outputs == 0
            && self.runtime.sink_deliveries == self.runtime.frames
            && self.runtime.fault_events > 0
            && self.runtime.offline_previews > 0
            && self.runtime.raw_fallbacks > 0;
        format!(
            concat!(
                "{{\"schema\":1,\"version\":\"0.29.0\",",
                "\"source\":\"single_integrated_offline_software_preflight\",",
                "\"adversarial_scenarios\":8,\"configurations_searched\":{},",
                "\"separate_training_seeds\":2,\"separate_holdout_seeds\":2,",
                "\"runtime_software_safety_pass\":{},",
                "\"synthetic_optimizer_gate\":{},",
                "\"candidate_automatically_enabled\":false,",
                "\"preview_output_sink\":\"memory_only\",",
                "\"physical_virtual_gamepad_backend\":\"unavailable_denied\",",
                "\"physical_hid_identity_verified\":false,",
                "\"real_controller_evidence\":false,",
                "\"real_rocket_league_gameplay_tested\":false,",
                "\"real_end_to_end_latency_measured\":false,",
                "\"physical_acceptance\":\"UNVERIFIED\",",
                "\"production_qualification\":\"BLOCKED\"}}\n"
            ),
            self.search.searched, pass, self.search.chosen.synthetic_gate_pass
        )
    }
    fn preflight() -> &'static str {
        concat!(
            "V0.29 WINDOWS INTEGRATION PREFLIGHT (not hardware certification)\n",
            "1. Native controller packet core: compiled and safety-tested.\n",
            "2. Buttons/triggers and secondary stick: preserved unchanged.\n",
            "3. Driver adapter: explicit unsupported mode (cannot write to games).\n",
            "4. Per-device epoch: locally monotonic session guard, NOT USB identity.\n",
            "5. Emergency off: invalidates calibration; no auto reconnect.\n",
            "6. Output inspector: memory only, no physical device mutation.\n",
            "7. Timing percentiles: coarse core-only histogram, NOT game latency.\n",
            "8. Virtual driver installation: NOT attempted.\n",
            "9. Real XInput controller: needs manual connection and consent.\n",
            "10. Rocket League compatibility and long physical endurance: UNTESTED.\n",
            "PRODUCTION ACCEPTANCE: BLOCKED.\n"
        )
    }
    pub fn publish(&self, out: &Path) -> io::Result<()> {
        if out.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "out exists"));
        }
        let parent = out.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let filename = out
            .file_name()
            .ok_or_else(|| io::Error::other("missing output name"))?;
        let tmp = parent.join(format!(
            ".{}.{}.{}.v29-tmp",
            filename.to_string_lossy(),
            std::process::id(),
            RUN_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&tmp)?;
        let result = (|| -> io::Result<()> {
            for (name, body) in [
                ("release_manifest.json", self.manifest()),
                ("adversarial_search.json", self.search.json()),
                ("runtime_metrics.json", self.runtime.json()),
                ("runtime_sample_trace.csv", self.runtime.trace.clone()),
                ("windows_preflight.txt", Self::preflight().to_string()),
            ] {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(tmp.join(name))?;
                file.write_all(body.as_bytes())?;
                file.sync_all()?;
            }
            #[cfg(windows)]
            {
                crate::capture::publish_new(&tmp, out)?;
            }
            #[cfg(not(windows))]
            {
                if out.exists() {
                    return Err(io::Error::new(io::ErrorKind::AlreadyExists, "out appeared"));
                }
                fs::rename(&tmp, out)?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(tmp);
        }
        result
    }
}
pub fn run(seed: u64, samples: usize, stress_frames: usize, out: &Path) -> io::Result<()> {
    let p = Package::evaluate(seed, samples, stress_frames)?;
    p.publish(out)?;
    println!("V0.29 SOFTWARE INTEGRATION: 8 hostile scenarios / {} parameter configs / {} packet stress frames",
        p.search.searched,p.runtime.frames);
    println!("CORE approximate p99 upper bucket={} ns, fault fallback={} / preview={} / writes_to_game=0",
        p.runtime.timing.bucket_quantile_upper_ns(99),
        p.runtime.raw_fallbacks,p.runtime.offline_previews);
    println!(
        "SYNTHETIC_OPTIMIZER_GATE={} LIVE_PROMOTION=false PHYSICAL_QUALIFICATION=BLOCKED",
        p.search.chosen.synthetic_gate_pass
    );
    println!("EVIDENCE={}", out.display());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_faults_and_emergency_off_survive_extended_stress() {
        let t = stress(20261029, 40000).unwrap();
        assert_eq!(t.frames, 40000);
        assert!(t.offline_previews > 0);
        assert!(t.raw_fallbacks > 0);
        assert!(t.fault_events > 0);
        assert!(t.disconnections > 0);
        assert!(t.emergency_stops > 0);
        assert_eq!(t.physical_device_writes, 0);
        assert_eq!(t.sink_deliveries, t.frames);
        assert!(t.json().contains("\"game_input_reached\":false"));
        assert_eq!(t.trace.lines().count(), 257);
    }
    #[test]
    fn run_artifacts_are_atomic_and_nonreplacing() {
        let root = std::env::temp_dir().join(format!(
            "ga-v29-{}-{}",
            std::process::id(),
            RUN_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let p = Package::evaluate(20261029, 5000, 10000).unwrap();
        assert!(p
            .manifest()
            .contains("\"production_qualification\":\"BLOCKED\""));
        assert!(p
            .manifest()
            .contains("\"runtime_software_safety_pass\":true"));
        p.publish(&root).unwrap();
        for name in [
            "release_manifest.json",
            "adversarial_search.json",
            "runtime_metrics.json",
            "runtime_sample_trace.csv",
            "windows_preflight.txt",
        ] {
            assert!(root.join(name).is_file());
        }
        assert!(p.publish(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn invalid_workloads_dont_publish_anything() {
        assert!(Package::evaluate(0, 10, 10000).is_err());
        assert!(Package::evaluate(0, 5000, 20).is_err());
    }
}
