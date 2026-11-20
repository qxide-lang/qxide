// Statistics of `qxide run --shots 1000 --seed S` over consecutive seeds S = 0..200, the path the
// CLI and the playground share. Before per-shot seeds were derived, `--seed S` simulated shot k
// with seed S + k, so nearby runs reused almost every shot: counts barely moved between seeds
// (Bell `00` had a standard deviation of 2.9 instead of 15.8). Seeds are fixed, so these tests
// are deterministic; the bounds are about 5 standard deviations wide.

use std::collections::HashMap;

use super::*;

const SEEDS: u64 = 200;
const SHOTS: u64 = 1000;

/// Measurement record (`[0, 1]`) -> count, from a multi-shot table.
fn counts(src: &str, seed: u64) -> HashMap<String, u64> {
    let out = run("t.qx", src, SHOTS, seed).unwrap_or_else(|Failure(d)| panic!("{d}"));
    out.lines()
        .filter(|l| !l.starts_with("  "))
        .map(|l| {
            let (head, count) = l.rsplit_once("  ×").expect("row has a count");
            let record = head.split("  ").next().unwrap().to_string();
            (record, count.parse().unwrap())
        })
        .collect()
}

/// Per-seed counts of each record, after checking that only `allowed` records occur.
fn sample(src: &str, allowed: &[&str]) -> HashMap<String, Vec<u64>> {
    let mut per: HashMap<String, Vec<u64>> = allowed
        .iter()
        .map(|r| (r.to_string(), Vec::new()))
        .collect();
    for seed in 0..SEEDS {
        let c = counts(src, seed);
        for record in c.keys() {
            assert!(
                allowed.contains(&record.as_str()),
                "seed {seed}: unexpected {record}"
            );
        }
        for (record, v) in per.iter_mut() {
            v.push(c.get(record).copied().unwrap_or(0));
        }
    }
    per
}

/// Checks that per-seed counts look like independent Binomial(SHOTS, p) draws: the mean, the
/// variance, and a chi-square dispersion statistic (about chi-square with SEEDS degrees of freedom).
fn assert_binomial(what: &str, counts: &[u64], p: f64) {
    let k = counts.len() as f64;
    let (np, var) = (SHOTS as f64 * p, SHOTS as f64 * p * (1.0 - p));
    let mean = counts.iter().sum::<u64>() as f64 / k;
    let sample_var = counts
        .iter()
        .map(|&c| (c as f64 - mean).powi(2))
        .sum::<f64>()
        / (k - 1.0);
    let chi2: f64 = counts.iter().map(|&c| (c as f64 - np).powi(2) / var).sum();
    let summary = format!("{what}: mean {mean:.2} (expected {np}), variance {sample_var:.1} (expected {var:.1}), chi2 {chi2:.1} on {k} df");
    assert!((mean - np).abs() <= 5.0 * (var / k).sqrt(), "{summary}");
    let ratio = sample_var / var;
    assert!((0.6..=1.5).contains(&ratio), "{summary}");
    // chi2 has mean k and standard deviation sqrt(2k) = 20 for k = 200.
    assert!(
        (k - 5.0 * (2.0 * k).sqrt()..=k + 5.0 * (2.0 * k).sqrt()).contains(&chi2),
        "{summary}"
    );
}

/// Pearson goodness of fit of the counts pooled over all seeds; `df` = categories - 1.
fn assert_fits(what: &str, observed: &[(u64, f64)], df: usize) {
    let total: u64 = observed.iter().map(|o| o.0).sum();
    let chi2: f64 = observed
        .iter()
        .map(|&(o, p)| (o as f64 - total as f64 * p).powi(2) / (total as f64 * p))
        .sum();
    // Far beyond the 99.999th percentile for 1 to 3 degrees of freedom (19.5, 23.0, 25.9).
    let bound = [0.0, 24.0, 28.0, 31.0][df];
    assert!(
        chi2 < bound,
        "{what}: chi2 {chi2:.2} on {df} df, counts {observed:?}"
    );
}

#[test]
fn bell_counts_are_binomial_across_seeds() {
    let src = include_str!("../../examples/qx/bell.qx");
    let per = sample(src, &["[0, 0]", "[1, 1]"]);
    assert_binomial("bell 00", &per["[0, 0]"], 0.5);
    let zeros: u64 = per["[0, 0]"].iter().sum();
    let ones: u64 = per["[1, 1]"].iter().sum();
    assert_eq!(zeros + ones, SEEDS * SHOTS);
    assert_fits("bell", &[(zeros, 0.5), (ones, 0.5)], 1);
}

#[test]
fn ghz_counts_are_binomial_across_seeds() {
    let src = include_str!("../../examples/qx/ghz.qx");
    let (zero, one) = ("[0, 0, 0, 0, 0]", "[1, 1, 1, 1, 1]");
    let per = sample(src, &[zero, one]);
    assert_binomial("ghz 00000", &per[zero], 0.5);
    let pooled = [(per[zero].iter().sum(), 0.5), (per[one].iter().sum(), 0.5)];
    assert_fits("ghz", &pooled, 1);
}

#[test]
fn rotation_with_probability_one_quarter_is_binomial_across_seeds() {
    // ry(PI / 3) gives P(1) = sin^2(PI / 6) = 0.25, on the state-vector backend.
    let src = "void main() {\n    Qubit q = qubit();\n    ry(q, PI / 3);\n    Bit m = measure(q);\n    println(\"{}\", m);\n}\n";
    let per = sample(src, &["[0]", "[1]"]);
    assert_binomial("ry 1", &per["[1]"], 0.25);
    let pooled = [
        (per["[0]"].iter().sum(), 0.75),
        (per["[1]"].iter().sum(), 0.25),
    ];
    assert_fits("ry", &pooled, 1);
}

#[test]
fn two_random_measurements_per_shot_are_independent() {
    // Two fair coins in one shot: all four records are equally likely, including on runs where
    // a shot's second draw could line up with the next shot's first draw.
    let src = "void main() {\n    QReg r = qreg(2);\n    h(r[0]);\n    h(r[1]);\n    BitReg m = measure(r);\n    println(\"{}\", m);\n}\n";
    let records = ["[0, 0]", "[0, 1]", "[1, 0]", "[1, 1]"];
    let per = sample(src, &records);
    assert_binomial("coins 00", &per["[0, 0]"], 0.25);
    let pooled: Vec<(u64, f64)> = records
        .iter()
        .map(|r| (per[*r].iter().sum(), 0.25))
        .collect();
    assert_fits("two coins", &pooled, 3);
}

#[test]
fn same_seed_gives_the_same_table() {
    let src = include_str!("../../examples/qx/bell.qx");
    for seed in [0, 1, 42] {
        assert_eq!(
            run("t.qx", src, 300, seed).ok(),
            run("t.qx", src, 300, seed).ok()
        );
    }
    assert_ne!(run("t.qx", src, 300, 0).ok(), run("t.qx", src, 300, 1).ok());
}
