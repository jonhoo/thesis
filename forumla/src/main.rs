const NUM: usize = 5000000;

fn main() {
    for &(skew, exp) in &[("80/20", 0.886), ("80/5", 0.99), ("90/1", 1.15)] {
        println!("\\hline");

        let mut first = true;
        let harmonic = harmonic(NUM, exp);
        for &rate in &[100_000, 1_000_000, 10_000_000] {
            let pct = |pt| {
                let mut p = 0.0;
                let mut k = 1;
                while p < pt && k <= NUM {
                    p += zipf(k, exp, harmonic);
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
            let nines_p = pct(0.99);
            let one_s = 100.0 * (est(1, rate, exp) / (NUM as f64));
            let thirty_s = 100.0 * (est(30, rate, exp) / (NUM as f64));

            let human_rate = format!("{}M/s", rate as f64 / 1_000_000.0);

            if first {
                println!(
                    "{} & {:.2} & {} & {:.1} & {:.1} & {:.1} \\\\",
                    skew, exp, human_rate, nines_p, one_s, thirty_s
                );
            } else {
                println!(
                    "& & {} & {:.1} & {:.1} & {:.1} \\\\",
                    human_rate, nines_p, one_s, thirty_s
                );
            }
            first = false;
        }
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
    (1..=NUM)
        .map(|k| 1.0 - (1.0 - zipf(k, exp, harmonic)).powf((t * rate) as f64))
        .sum()
}
