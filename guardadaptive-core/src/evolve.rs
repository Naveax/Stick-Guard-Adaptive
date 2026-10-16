//! Evolutionary red-team stress generator; never used by the production live filter.
//! Fitness is evaluated against simulated truth, which is strictly withheld from the guard.
use crate::{
    resolution::Resolution,
    sweep::{evaluate_with_resolution, Spec},
};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::Instant,
};

#[derive(Clone, Copy)]
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn fraction(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }
    fn signed(&mut self) -> f64 {
        self.fraction() * 2.0 - 1.0
    }
}
fn mutate(mut s: Spec, rng: &mut Rng) -> Spec {
    match rng.next() % 7 {
        0 => {
            s.amplitude = (s.amplitude.ln() + rng.signed() * 1.7)
                .clamp(0.00001_f64.ln(), 0.0)
                .exp();
        }
        1 => {
            s.direction = (s.direction + rng.signed() * std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU);
        }
        2 => {
            s.transition = (rng.next() % 8) as u8;
        }
        3 => {
            s.intent = (rng.next() % 9) as u8;
        }
        4 => {
            s.noise = [0., 0.00001, 0.0005, 0.01][(rng.next() % 4) as usize];
        }
        5 => {
            s.amplitude = [0.00001, 0.0001, 0.01, 0.1, 0.5, 1.0][(rng.next() % 6) as usize];
        }
        _ => {
            s.transition = (rng.next() % 8) as u8;
            s.intent = (rng.next() % 9) as u8;
        }
    }
    s.amplitude = s.amplitude.clamp(0.00001, 1.0);
    s.case_id = rng.next();
    s.seed = rng.next();
    s
}
#[derive(Clone, Copy)]
struct Individual {
    score: f64,
    spec: Spec,
}
pub fn run(
    seed: u64,
    generations: usize,
    population: usize,
    frames: usize,
    out: &Path,
) -> io::Result<()> {
    run_with_resolution(
        seed,
        generations,
        population,
        frames,
        out,
        Resolution::Ideal,
    )
}
pub fn run_with_resolution(
    seed: u64,
    generations: usize,
    population: usize,
    frames: usize,
    out: &Path,
    resolution: Resolution,
) -> io::Result<()> {
    let generations = generations.clamp(1, 100000);
    let population = population.clamp(4, 256);
    let frames = frames.clamp(256, 65536);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(out)?;
    let mut csv = io::BufWriter::new(file);
    writeln!(csv,"generation,ordinal,fitness,amplitude_pct,direction_degrees,transition,intent,noise_pct,seed,case_id,neutral_rmse,motion_rmse,micro_lost,wrong_direction,false_positive,resolution,below_report_resolution")?;
    let mut rng = Rng::new(seed);
    let mut elite: Vec<Individual> = Vec::with_capacity(32);
    let started = Instant::now();
    let mut evaluated = 0usize;
    for generation in 0..generations {
        for ordinal in 0..population {
            let spec = if elite.is_empty() || ordinal % 5 == 0 {
                Spec::randomized(rng.next(), rng.next())
            } else {
                let p = elite[(rng.next() as usize) % elite.len()].spec;
                mutate(p, &mut rng)
            };
            let result = evaluate_with_resolution(spec, frames, resolution);
            let motion_n = result.guard.n_motion.max(1) as f64;
            let score = result.guard.objective()
                + (result.guard.wrong_direction as f64 / motion_n) * 0.25
                + (result.guard.false_positive as f64 / result.guard.n_neutral.max(1) as f64)
                    * 0.12;
            writeln!(
                csv,
                "{},{},{:.9},{:.9},{:.5},{},{},{:.8},{},{},{:.8},{:.8},{},{},{},{},{}",
                generation,
                ordinal,
                score,
                spec.amplitude * 100.0,
                spec.direction.to_degrees(),
                spec.transition,
                spec.intent,
                spec.noise * 100.0,
                spec.seed,
                spec.case_id,
                result.guard.neutral_rmse(),
                result.guard.motion_rmse(),
                result.guard.micro_lost,
                result.guard.wrong_direction,
                result.guard.false_positive,
                resolution.name(),
                result.below_report_resolution
            )?;
            elite.push(Individual { score, spec });
            elite.sort_by(|a, b| b.score.total_cmp(&a.score));
            elite.truncate(24);
            evaluated += 1;
        }
    }
    csv.flush()?;
    println!("EVOLUTIONARY ATTACK COMPLETE generations={generations} population={population} evaluated={evaluated} frames={frames} elapsed_ms={} CSV={}",
        started.elapsed().as_millis(),out.display());
    for (i, w) in elite.iter().take(8).enumerate() {
        println!("WORST #{:02}: score={:.8} amp={:.8}% angle={:.3} transition={} intent={} noise={:.6}% seed={} case_id={}",
            i+1,w.score,w.spec.amplitude*100.0,w.spec.direction.to_degrees(),
            w.spec.transition,w.spec.intent,w.spec.noise*100.0,w.spec.seed,w.spec.case_id);
    }
    println!("Evolution report model={} positive_lsb={:.10}; synthetic counterexamples are not evidence of real-controller performance.",resolution.name(),resolution.positive_lsb());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutant_stays_in_legal_envelope() {
        let mut rng = Rng::new(42);
        let mut s = Spec::randomized(8, 5);
        for _ in 0..1000 {
            s = mutate(s, &mut rng);
            assert!((0.00001..=1.0).contains(&s.amplitude));
            assert!((0.0..std::f64::consts::TAU).contains(&s.direction));
            assert!(s.transition < 8 && s.intent < 9 && s.noise <= 0.01);
        }
    }
}
