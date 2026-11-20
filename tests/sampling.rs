// Per-shot seeds: `Circuit::simulate_shots` and `qxide::shot_seed`.

use std::collections::HashSet;

use qxide::{shot_seed, Backend, Circuit};

#[test]
fn simulate_shots_uses_shot_seed_for_every_shot() {
    let mut c = Circuit::new();
    let mut q = c.qubit();
    c.h(&mut q);
    let m = c.measure(q);
    for backend in [Backend::StateVector, Backend::Stabilizer] {
        let shots: Vec<bool> = c
            .simulate_shots(backend, 7, 64)
            .map(|s| s.unwrap().get(m))
            .collect();
        let manual: Vec<bool> = (0..64)
            .map(|k| c.simulate_with(backend, shot_seed(7, k)).unwrap().get(m))
            .collect();
        assert_eq!(shots, manual, "{backend:?}");
        assert!(shots.contains(&true) && shots.contains(&false));
    }
}

#[test]
fn shot_seeds_do_not_repeat_across_nearby_runs() {
    // The old scheme gave shot k of run S the seed S + k, so runs S and S + 1 shared all but one shot.
    let mut seen = HashSet::new();
    for seed in 0..200 {
        for shot in 0..1000 {
            assert!(
                seen.insert(shot_seed(seed, shot)),
                "seed {seed} shot {shot}"
            );
        }
    }
}

#[test]
fn consecutive_shots_are_uncorrelated() {
    // Each shot measures two fair coins. Compare a shot's second bit with the next shot's first
    // bit: with independent shots they agree half the time.
    let mut c = Circuit::new();
    let (mut a, mut b) = (c.qubit(), c.qubit());
    c.h(&mut a);
    c.h(&mut b);
    let (ma, mb) = (c.measure(a), c.measure(b));
    for backend in [Backend::StateVector, Backend::Stabilizer] {
        let n = 40_000u64;
        let shots: Vec<(bool, bool)> = c
            .simulate_shots(backend, 0, n)
            .map(|s| {
                let s = s.unwrap();
                (s.get(ma), s.get(mb))
            })
            .collect();
        let agree = shots.windows(2).filter(|w| w[0].1 == w[1].0).count() as f64;
        let pairs = (n - 1) as f64;
        // 5 standard deviations of a Binomial(pairs, 0.5) count.
        assert!(
            (agree - pairs / 2.0).abs() < 5.0 * (pairs / 4.0).sqrt(),
            "{backend:?}: {agree} of {pairs}"
        );
        let both = shots.iter().filter(|s| s.0 && s.1).count() as f64;
        assert!(
            (both - n as f64 / 4.0).abs() < 5.0 * (n as f64 * 0.1875).sqrt(),
            "{backend:?}: {both} of {n}"
        );
    }
}
