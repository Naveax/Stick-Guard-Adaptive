use guardadaptive::resolution::Resolution;
use std::{env, path::PathBuf, process};
fn usage() -> ! {
    eprintln!("Usage:");
    eprintln!("  guardadaptive sweep --cases 70000 --frames 512 --seed 1337 --out D:\\GuardAdaptiveReports\\sweep.csv");
    eprintln!("  guardadaptive replay --case 69120 --frames 512 --seed 1337 --resolution xinput16");
    eprintln!("  --resolution ideal | xinput16 | signed12 | signed8 (sweep/replay/evolve)");
    eprintln!("  guardadaptive coach --out reports/coach_steps.csv");
    eprintln!("  guardadaptive devices");
    eprintln!("  guardadaptive v68-record-compare --input TRAIN_RAW.csv --reference TRAIN_INTENT.csv --first HOLDOUT_RAW.csv --second HOLDOUT_INTENT.csv --stick left --out NEW.json");
    eprintln!("  guardadaptive v67-dataset-split --input TRAIN_RAW.csv --reference TRAIN_INTENT.csv --first HOLDOUT_RAW.csv --second HOLDOUT_INTENT.csv --stick left --out NEW.json");
    eprintln!("  guardadaptive v74-bounded-research --out NEW.json");
    eprintln!("  guardadaptive v73-blind-fence --samples 1024 --out NEW.json");
    eprintln!("  guardadaptive v66-blind-review --out NEW.json");
    eprintln!("  guardadaptive v65-host-clock --out NEW.json");
    eprintln!("  guardadaptive v65-fuzz-packets --samples 100000 --seed 20261014 --out NEW.json");
    eprintln!("  guardadaptive v65-crash-child --input NEW_EMPTY_LAB_DIRECTORY");
    eprintln!("  guardadaptive v65-restart-check --input LAB_DIR --out NEW_RECEIPT.json");
    eprintln!(
        "  guardadaptive v65-reset-confirm --input LAB_DIR --acknowledge CONFIRM_OFFLINE_RESET_V65"
    );
    eprintln!("  guardadaptive v64-concurrency --samples 256 --out concurrency-NEW.json");
    eprintln!("  guardadaptive v63-counter-limits --samples 8192 --out limits-NEW.json");
    eprintln!("  guardadaptive v62-epoch-audit --samples 8192 --out epoch-audit-NEW.json");
    eprintln!("  guardadaptive v61-backend-counter --samples 8192 --out backend-counter-NEW.json");
    eprintln!("  guardadaptive v60-report-replay --samples 8192 --out replay-NEW.json");
    eprintln!("  guardadaptive v59-freshness-audit --samples 8192 --out freshness-NEW.json");
    eprintln!("  guardadaptive v54-provider-fault --samples 36864 --out provider-fault-NEW.json");
    eprintln!("  guardadaptive v46-lifecycle-audit --samples 45056 --out lifecycle-NEW.json");
    eprintln!("  guardadaptive v45-provider-audit --input pnp-inventory.txt --out provider-audit-NEW.json");
    eprintln!("  guardadaptive v43-driver-contract --samples 65536 --out D:\\GuardAdaptiveReports\\v43-driver-contract-NEW.json");
    eprintln!("  guardadaptive v42-neutral-trend --input WINDOWS.csv --out new-report.json");
    eprintln!("  guardadaptive v42-neutral-suite --seed 20261013 --out new-suite.json");
    eprintln!("  guardadaptive v41-reference-audit --input controller.csv [--reference intent.csv] --stick left --out D:\\GuardAdaptiveReports\\reference-NEW.json");
    eprintln!(
        "  guardadaptive v40-drift-holdout --out D:\\GuardAdaptiveReports\\v40-drift-NEW.json"
    );
    eprintln!("  guardadaptive v39-stress --samples 262144 --seed 20261013 --out D:\\GuardAdaptiveReports\\v39-stress-NEW.json");
    eprintln!("  guardadaptive v38-supervisor --samples 100000 --out D:\\GuardAdaptiveReports\\v38-supervisor-NEW.json");
    eprintln!("  guardadaptive v35-bridge --samples 10000 --out D:\\GuardAdaptiveReports\\v35-bridge-NEW.json");
    eprintln!("  guardadaptive v34-recenter --seed 20261034 --samples 6400 --out D:\\GuardAdaptiveReports\\v34-new-evidence.json");
    eprintln!("  guardadaptive v32-pipeline --samples 100000 --out D:\\GuardAdaptiveReports\\v32-pipeline-NEW.json");
    eprintln!("  guardadaptive profile-export --input profiles --out D:\\GuardAdaptiveReports\\backup-NEW");
    eprintln!("  guardadaptive profile-restore --input BACKUP_DIR --out D:\\GuardAdaptiveReports\\restored-NEW");
    eprintln!("  guardadaptive profile-inventory --input profiles --out D:\\GuardAdaptiveReports\\inventory-NEW.json");
    eprintln!("  guardadaptive route-audit --out D:\\GuardAdaptiveReports\\route-guard-NEW.json");
    eprintln!("  guardadaptive monitor-readonly --slot 0 --samples 256 --period-ms 8 --out D:\\GuardAdaptiveReports\\monitor-new.json");
    eprintln!("  guardadaptive diagnose --slot 0 --samples 256 --period-ms 8 --out reports/xinput_diagnostic.json");
    eprintln!("  guardadaptive neutral-audit --first first.csv --second second.csv --stick left --acknowledge TWO_RELEASES --out reports/neutral_audit.json");
    eprintln!("  guardadaptive neutral-session --slot 0 --stick left --period-ms 8 --out D:\\GuardAdaptiveReports\\neutral_session_NEW");
    eprintln!("  guardadaptive session-replay --input SESSION_DIR --stick left --out replay.json [--reference OTHER_SESSION_DIR]");
    eprintln!(
        "  guardadaptive session-dataset --input SESSION_DIR --stick left --out NEW_DATASET_DIR"
    );
    eprintln!(
        "  guardadaptive session-benchmark --input SESSION_DIR --stick left --out NEW_REPORT.json"
    );
    eprintln!(
        "  guardadaptive truth-oracle --seed 20261024 --samples 12000 --out synthetic_truth.json"
    );
    eprintln!("  guardadaptive quality-matrix --seed 20261025 --samples 12000 --out quality_blockers.json");
    eprintln!("  guardadaptive shadow-session --input SESSION_DIR --stick left --out trace.csv --report trace.json");
    eprintln!("  guardadaptive v29-all --seed 20261029 --samples 10000 --soak-frames 200000 --out D:\\GuardAdaptiveReports\\v29-release-NEW");
    eprintln!("  guardadaptive v29-adversarial --seed 20261029 --samples 10000 --out D:\\GuardAdaptiveReports\\v29-adversarial-NEW.json");
    eprintln!("  guardadaptive v28-diagnose --seed 20261028 --samples 72000 --out D:\\GuardAdaptiveReports\\v28-regimes-NEW.json");
    eprintln!("  guardadaptive v27-all --seed 20261027 --samples 12000 --soak-frames 200000 --out D:\\GuardAdaptiveReports\\v27-evidence-NEW [--input PHYSICAL_SESSION_DIR] [--reference EXTERNAL_DECLARED_REFERENCE_CSV]");
    eprintln!("  guardadaptive ui --port 43137");
    eprintln!("  guardadaptive capture --slot 0 --samples 256 --period-ms 8 --out reports/controller_readonly.csv");
    eprintln!("  guardadaptive preview --slot 0 --stick left --device-id MY-CONTROLLER --input profiles/left.profile --samples 128 --period-ms 8");
    eprintln!("  guardadaptive guided-live --slot 0 --stick left --device-id MY-CONTROLLER --period-ms 8 --out profiles/left.profile");
    eprintln!("  guardadaptive guided-demo --out D:\\GuardAdaptiveReports\\v06_demo.profile");
    eprintln!("  guardadaptive profile-check --input D:\\GuardAdaptiveReports\\v06_demo.profile --device-id SYNTHETIC-DEMO-001 --stick left --resolution xinput16");
    eprintln!("  guardadaptive reacquire-demo --out D:\\GuardAdaptiveReports\\reacquire_demo.csv");
    eprintln!("  guardadaptive evolve --generations 250 --population 32 --frames 1024 --seed 1337");
    eprintln!("  guardadaptive assess --input captures/user_session.csv");
    eprintln!("  guardadaptive coach-demo --out reports/simulated_user_session.csv");
    eprintln!("  guardadaptive train-profile --input captures/controller.csv --out profiles/controller.toml");
    eprintln!("  guardadaptive qualify --input D:\\GuardAdaptiveReports\\full_matrix_20261011.csv");
    eprintln!("  guardadaptive stress --seed 1337 --samples 12000 --out reports/stress.csv");
    eprintln!("  guardadaptive attack --trials 100 --seed 1337 --samples 6000");
    eprintln!("  guardadaptive trace --scenario random_gauntlet --seed 1337 --samples 12000");
    eprintln!("  guardadaptive bench --samples 500000");
    process::exit(2)
}
fn main() {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("stress");
    let mut seed = 1337_u64;
    let mut samples = 12000_usize;
    let mut capture_count: Option<usize> = None;
    let mut slot = 0_u32;
    let mut port = 43137_u16;
    let mut period_ms = 8_u64;
    let mut trials = 100_usize;
    let mut generations = 250_usize;
    let mut population = 32_usize;
    let mut cases = 69120_u64;
    let mut frames = 512_usize;
    let mut single_case = 0_u64;
    let mut resolution = Resolution::Ideal;
    let mut out: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut reference: Option<PathBuf> = None;
    let mut shadow_report: Option<PathBuf> = None;
    let mut soak_frames: usize = 200000;
    let mut first: Option<PathBuf> = None;
    let mut second: Option<PathBuf> = None;
    let mut acknowledgement: String = String::new();
    let mut scenario = String::from("random_gauntlet");
    let mut device_id: Option<String> = None;
    let mut stick = String::from("left");
    let mut pos = 2;
    while pos < args.len() {
        let key = &args[pos];
        let value = args.get(pos + 1).unwrap_or_else(|| usage());
        match key.as_str() {
            "--seed" => seed = value.parse().unwrap_or_else(|_| usage()),
            "--soak-frames" => soak_frames = value.parse().unwrap_or_else(|_| usage()),
            "--samples" => {
                samples = value.parse().unwrap_or_else(|_| usage());
                capture_count = Some(samples);
            }
            "--slot" => slot = value.parse().unwrap_or_else(|_| usage()),
            "--port" => port = value.parse().unwrap_or_else(|_| usage()),
            "--period-ms" => period_ms = value.parse().unwrap_or_else(|_| usage()),
            "--trials" => trials = value.parse().unwrap_or_else(|_| usage()),
            "--generations" => generations = value.parse().unwrap_or_else(|_| usage()),
            "--population" => population = value.parse().unwrap_or_else(|_| usage()),
            "--cases" => cases = value.parse().unwrap_or_else(|_| usage()),
            "--frames" => frames = value.parse().unwrap_or_else(|_| usage()),
            "--case" => single_case = value.parse().unwrap_or_else(|_| usage()),
            "--resolution" => resolution = Resolution::parse(value).unwrap_or_else(|| usage()),
            "--scenario" => scenario = value.clone(),
            "--device-id" => device_id = Some(value.clone()),
            "--stick" => stick = value.clone(),
            "--out" => out = Some(PathBuf::from(value)),
            "--input" => input = Some(PathBuf::from(value)),
            "--reference" => reference = Some(PathBuf::from(value)),
            "--report" => shadow_report = Some(PathBuf::from(value)),
            "--first" => first = Some(PathBuf::from(value)),
            "--second" => second = Some(PathBuf::from(value)),
            "--acknowledge" => acknowledgement = value.clone(),
            _ => usage(),
        }
        pos += 2;
    }
    fn fail(e: std::io::Error) -> ! {
        eprintln!("error: {e}");
        process::exit(1)
    }
    match cmd {
        "devices" => guardadaptive::capture::devices().unwrap_or_else(|e| fail(e)),
        "v68-record-compare" => {
            let parsed_stick = match stick.as_str() {
                "left" => guardadaptive::profile_store::Stick::Left,
                "right" => guardadaptive::profile_store::Stick::Right,
                _ => usage(),
            };
            guardadaptive::record_compare_v68::run(
                &input.unwrap_or_else(|| usage()),
                reference.as_deref(),
                &first.unwrap_or_else(|| usage()),
                second.as_deref(),
                parsed_stick,
                &out.unwrap_or_else(|| usage()),
            )
            .unwrap_or_else(|e| fail(e))
        }
        "v67-dataset-split" => {
            let parsed_stick = match stick.as_str() {
                "left" => guardadaptive::profile_store::Stick::Left,
                "right" => guardadaptive::profile_store::Stick::Right,
                _ => usage(),
            };
            guardadaptive::dataset_split_v67::run(
                &input.unwrap_or_else(|| usage()),
                reference.as_deref(),
                &first.unwrap_or_else(|| usage()),
                second.as_deref(),
                parsed_stick,
                &out.unwrap_or_else(|| usage()),
            )
            .unwrap_or_else(|e| fail(e))
        }
        "v74-bounded-research" => {
            guardadaptive::conservative_research_v74::run(&out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v73-blind-fence" => {
            guardadaptive::blind_fence_v73::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v66-blind-review" => guardadaptive::blind_review_v66::run(&out.unwrap_or_else(|| usage()))
            .unwrap_or_else(|e| fail(e)),
        "v65-host-clock" => guardadaptive::host_clock_v65::run(&out.unwrap_or_else(|| usage()))
            .unwrap_or_else(|e| fail(e)),
        "v65-fuzz-packets" => {
            guardadaptive::offline_fuzz_v65::run(samples, seed, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v65-crash-child" => {
            guardadaptive::crash_recovery_v65::crash_child(&input.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v65-restart-check" => guardadaptive::crash_recovery_v65::report(
            &input.unwrap_or_else(|| usage()),
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "v65-reset-confirm" => guardadaptive::crash_recovery_v65::operator_reset(
            &input.unwrap_or_else(|| usage()),
            &acknowledgement,
        )
        .unwrap_or_else(|e| fail(e)),
        "v64-concurrency" => {
            guardadaptive::concurrent_watch_v64::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v63-counter-limits" => {
            guardadaptive::authorization_bounds_v63::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v62-epoch-audit" => {
            guardadaptive::backend_epoch_v62::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v61-backend-counter" => {
            guardadaptive::backend_counter_v61::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v60-report-replay" => {
            guardadaptive::provider_replay_v60::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v59-freshness-audit" => {
            guardadaptive::provider_freshness_v59::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v54-provider-fault" => {
            guardadaptive::provider_fault_v54::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v46-lifecycle-audit" => {
            guardadaptive::lifecycle_v46::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v45-provider-audit" => guardadaptive::provider_audit_v45::run(
            &input.unwrap_or_else(|| usage()),
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "v43-driver-contract" => {
            guardadaptive::driver_contract_v43::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v42-neutral-trend" => guardadaptive::neutral_trend_v42::analyze(
            &input.unwrap_or_else(|| usage()),
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "v42-neutral-suite" => {
            guardadaptive::neutral_trend_v42::suite(seed, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v41-reference-audit" => {
            let parsed_stick = match stick.as_str() {
                "left" => guardadaptive::profile_store::Stick::Left,
                "right" => guardadaptive::profile_store::Stick::Right,
                _ => usage(),
            };
            guardadaptive::reference_gate_v41::audit(
                &input.unwrap_or_else(|| usage()),
                reference.as_deref(),
                parsed_stick,
                &out.unwrap_or_else(|| usage()),
            )
            .unwrap_or_else(|e| fail(e))
        }
        "v40-drift-holdout" => {
            guardadaptive::drift_holdout_v40::run(&out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v39-stress" => {
            guardadaptive::stress_v39::run(samples, seed, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v38-supervisor" => {
            guardadaptive::output_supervisor::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v35-bridge" => {
            guardadaptive::recenter_bridge::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v34-recenter" => {
            guardadaptive::operator_recenter::run(seed, samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v32-pipeline" => {
            guardadaptive::pipeline_v32::run(samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "profile-export" => guardadaptive::profile_archive::run_export(
            &input.unwrap_or_else(|| usage()),
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "profile-restore" => guardadaptive::profile_archive::run_restore(
            &input.unwrap_or_else(|| usage()),
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "profile-inventory" => guardadaptive::profile_inventory::run(
            &input.unwrap_or_else(|| usage()),
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "route-audit" => guardadaptive::truth_lab::save_new(
            &out.unwrap_or_else(|| usage()),
            &guardadaptive::routing_guard::report(),
        )
        .unwrap_or_else(|e| fail(e)),
        "monitor-readonly" => guardadaptive::live_monitor::run(
            slot,
            capture_count.unwrap_or(256),
            period_ms,
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "v29-all" => guardadaptive::software_suite::run(
            seed,
            samples,
            soak_frames,
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "v29-adversarial" => {
            guardadaptive::adversarial::run(seed, samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "v28-diagnose" => guardadaptive::truth_lab::diagnose_command(
            seed,
            samples,
            &out.unwrap_or_else(|| usage()),
        )
        .unwrap_or_else(|e| fail(e)),
        "v27-all" => {
            use guardadaptive::profile_store::Stick;
            let selected = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            guardadaptive::v27_all::run(
                seed,
                samples,
                soak_frames,
                input.as_deref(),
                reference.as_deref(),
                selected,
                &out.unwrap_or_else(|| usage()),
            )
            .unwrap_or_else(|e| fail(e));
        }
        "truth-oracle" => {
            guardadaptive::truth_lab::run(seed, samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "quality-matrix" => {
            guardadaptive::acceptance::run(seed, samples, &out.unwrap_or_else(|| usage()))
                .unwrap_or_else(|e| fail(e))
        }
        "shadow-session" => {
            use guardadaptive::profile_store::Stick;
            let which = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            guardadaptive::shadow::run(
                &input.unwrap_or_else(|| usage()),
                which,
                &out.unwrap_or_else(|| usage()),
                &shadow_report.unwrap_or_else(|| usage()),
            )
            .unwrap_or_else(|e| fail(e));
        }
        "session-dataset" | "session-benchmark" => {
            use guardadaptive::profile_store::Stick;
            let selected = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            let source = input.unwrap_or_else(|| usage());
            let target = out.unwrap_or_else(|| usage());
            if cmd == "session-dataset" {
                guardadaptive::measured_lab::dataset_command(&source, selected, &target)
                    .unwrap_or_else(|e| fail(e));
            } else {
                guardadaptive::measured_lab::benchmark_command(
                    &source,
                    selected,
                    reference.as_deref(),
                    &target,
                )
                .unwrap_or_else(|e| fail(e));
            }
        }
        "session-replay" => {
            use guardadaptive::profile_store::Stick;
            let selected = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            guardadaptive::session_replay::run(
                &input.unwrap_or_else(|| usage()),
                selected,
                reference.as_deref(),
                &out.unwrap_or_else(|| PathBuf::from("reports/session_replay.json")),
            )
            .unwrap_or_else(|e| fail(e));
        }
        "neutral-session" => {
            use guardadaptive::profile_store::Stick;
            let which = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            guardadaptive::neutral_session::native(
                slot,
                which,
                period_ms,
                &out.unwrap_or_else(|| PathBuf::from("reports/neutral_session_NEW")),
            )
            .unwrap_or_else(|e| fail(e));
        }
        "neutral-audit" => {
            use guardadaptive::profile_store::Stick;
            let which = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            guardadaptive::neutral_audit::run(
                &first.unwrap_or_else(|| usage()),
                &second.unwrap_or_else(|| usage()),
                which,
                &acknowledgement,
                &out.unwrap_or_else(|| PathBuf::from("reports/neutral_audit.json")),
            )
            .unwrap_or_else(|e| fail(e));
        }
        "diagnose" => guardadaptive::diagnostics::native(
            slot,
            capture_count.unwrap_or(256),
            period_ms,
            &out.unwrap_or_else(|| PathBuf::from("reports/xinput_diagnostic.json")),
        )
        .unwrap_or_else(|e| fail(e)),
        "ui" => guardadaptive::ui::run(port).unwrap_or_else(|e| fail(e)),
        "capture" => guardadaptive::capture::live(
            slot,
            capture_count.unwrap_or(256),
            period_ms,
            &out.unwrap_or_else(|| PathBuf::from("reports/controller_readonly.csv")),
        )
        .unwrap_or_else(|e| fail(e)),
        "preview" => {
            use guardadaptive::profile_store::{Identity, Stick};
            let which = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            let id = Identity::new(
                device_id.as_deref().unwrap_or_else(|| usage()),
                which,
                resolution,
            )
            .unwrap_or_else(|e| fail(e));
            guardadaptive::preview::native(
                slot,
                &input.unwrap_or_else(|| usage()),
                &id,
                capture_count.unwrap_or(128),
                period_ms,
            )
            .unwrap_or_else(|e| fail(e));
        }
        "guided-live" => {
            use guardadaptive::profile_store::Stick;
            let which = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            guardadaptive::guided_live::run(
                slot,
                which,
                device_id.as_deref().unwrap_or_else(|| usage()),
                period_ms,
                &out.unwrap_or_else(|| PathBuf::from("profiles/manual_xinput.profile")),
            )
            .unwrap_or_else(|e| fail(e));
        }
        "sweep" => guardadaptive::sweep::run_with_resolution(
            seed,
            cases,
            frames,
            &out.unwrap_or_else(|| PathBuf::from("reports/sweep.csv")),
            resolution,
        )
        .unwrap_or_else(|e| fail(e)),
        "replay" => {
            guardadaptive::sweep::replay_with_resolution(seed, single_case, frames, resolution)
        }
        "evolve" => guardadaptive::evolve::run_with_resolution(
            seed,
            generations,
            population,
            frames,
            &out.unwrap_or_else(|| PathBuf::from("reports/evolution.csv")),
            resolution,
        )
        .unwrap_or_else(|e| fail(e)),
        "guided-demo" => guardadaptive::guided::demo(
            &out.unwrap_or_else(|| PathBuf::from("reports/guided_demo.profile")),
        )
        .unwrap_or_else(|e| fail(e)),
        "profile-check" => {
            use guardadaptive::profile_store::{Identity, Stick};
            let which = match stick.as_str() {
                "left" => Stick::Left,
                "right" => Stick::Right,
                _ => usage(),
            };
            let id = Identity::new(
                device_id.as_deref().unwrap_or_else(|| usage()),
                which,
                resolution,
            )
            .unwrap_or_else(|e| fail(e));
            let path = input.unwrap_or_else(|| usage());
            let v = guardadaptive::profile_store::load(&path, &id).unwrap_or_else(|e| fail(e));
            println!("VALID PROFILE: {} device={} stick={} resolution={} center=({:.8},{:.8}) agreement={:.8} MAD=({:.8},{:.8})",
                path.display(),id.device_id,id.stick.name(),id.resolution.name(),
                v.center().x,v.center().y,v.agreement,v.neutral_mad.x,v.neutral_mad.y);
            println!("The checksum is for accidental corruption, not malicious tamper-proofing.");
        }
        "reacquire-demo" => guardadaptive::reacquire::demo(
            &out.unwrap_or_else(|| PathBuf::from("reports/reacquire_demo.csv")),
        )
        .unwrap_or_else(|e| fail(e)),
        "coach" => guardadaptive::coach::plan(
            &out.unwrap_or_else(|| PathBuf::from("reports/coach_steps.csv")),
        )
        .unwrap_or_else(|e| fail(e)),
        "coach-demo" => guardadaptive::coach::demo(
            &out.unwrap_or_else(|| PathBuf::from("reports/simulated_user_session.csv")),
        )
        .unwrap_or_else(|e| fail(e)),
        "qualify" => match guardadaptive::qualify::run(&input.unwrap_or_else(|| usage())) {
            Ok(true) => {}
            Ok(false) => process::exit(3),
            Err(e) => fail(e),
        },
        "train-profile" => guardadaptive::profile::train(
            &input.unwrap_or_else(|| usage()),
            &out.unwrap_or_else(|| PathBuf::from("profiles/controller.toml")),
        )
        .unwrap_or_else(|e| fail(e)),
        "assess" => guardadaptive::coach::assess(&input.unwrap_or_else(|| usage()))
            .unwrap_or_else(|e| fail(e)),
        "stress" => {
            guardadaptive::sim::run(
                seed,
                samples,
                &out.unwrap_or_else(|| PathBuf::from("reports/stress.csv")),
            )
            .unwrap_or_else(|e| fail(e));
        }
        "attack" => guardadaptive::sim::attack_search(
            seed,
            trials,
            samples,
            &out.unwrap_or_else(|| PathBuf::from("reports/attack.csv")),
        )
        .unwrap_or_else(|e| fail(e)),
        "trace" => guardadaptive::sim::export_trace(
            seed,
            samples,
            &scenario,
            &out.unwrap_or_else(|| PathBuf::from("reports/trace.csv")),
        )
        .unwrap_or_else(|e| fail(e)),
        "bench" => println!(
            "Algorithm throughput: {:.2} ns/sample (synthetic core-only microbenchmark)",
            guardadaptive::sim::benchmark(samples.max(1))
        ),
        _ => usage(),
    }
}
