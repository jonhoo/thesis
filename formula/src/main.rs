const NUM: usize = 5000000;

fn main() {
    println!("skew\talpha\tthroughput\tpercentage");
    let rates = [
        5_000, 10_000, 20_000, 50_000, 100_000, 250_000, 500_000, 750_000, 1_000_000, 2_000_000,
    ];

    // How large a fraction is access in "one eviction period"?
    // Noria evicts once per second, but keeping in mind that eviction may take some time
    // and such, let's overestimate memory use so we don't overstate Noria's performance.
    // So we use an eviction period of 2 seconds.
    let period = 2;

    for &(skew, alpha) in &[("80/20", 0.886), ("80/5", 0.99), ("90/1", 1.15)] {
        let harmonic = harmonic(NUM, alpha);
        for &rate in &rates {
            let _pct = |pt| {
                let mut p = 0.0;
                let mut k = 1;
                while p < pt && k <= NUM {
                    p += zipf(k, alpha, harmonic);
                    k += 1;
                }
                // println!(
                //     "the first {} articles ({:.1}%) make up {:.1}% of requests",
                //     k,
                //     100.0 * k as f64 / NUM as f64,
                //     100.0 * p
                // );
                100.0 * k as f64 / NUM as f64
            };

            // let eighty_p = pct(0.8);
            // let nines_p = pct(0.99);

            let one_eviction_period = 100.0 * est(period, rate, alpha);
            println!("{}\t{:.3}\t{}\t{}", skew, alpha, rate, one_eviction_period);
        }
    }
    for &rate in &rates {
        let p = 1.0 - 1.0 / NUM as f64;
        let p = p.powf((period * rate) as f64);
        let one_eviction_period: f64 = 1.0 - p;
        let one_eviction_period = 100.0 * one_eviction_period;
        println!(
            "{}\t{:.3}\t{}\t{}",
            "uniform", "NA", rate, one_eviction_period
        );
    }
}

#[allow(non_snake_case)]
fn harmonic(N: usize, s: f64) -> f64 {
    (1..=N).map(|n| 1.0 / (n as f64).powf(s)).sum()
}

fn zipf(k: usize, s: f64, harmonic: f64) -> f64 {
    (1.0 / (k as f64).powf(s)) / harmonic
}

fn est(t: usize, rate: usize, exp: f64) -> f64 {
    let harmonic = harmonic(NUM, exp);
    // NOTE: this _could_ use powi, but powf is twice as fast for some reason...
    let samples = (t * rate) as f64;
    let p: f64 = (1..=NUM)
        .map(|k| (1.0 - zipf(k, exp, harmonic)).powf(samples))
        .sum();
    1.0 - p / (NUM as f64)
}
