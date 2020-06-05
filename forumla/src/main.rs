const NUM: usize = 5000000;
const RATE: usize = 800000;
const EXP: f64 = 1.08;

fn main() {
    for &t in &[32, 64, 128, 256, 512] {
        println!("after {}s", t);
        let integral_w = est(t, 0.05);
        println!(
            ".. {}/{} articles voted on ({:.1}%)",
            integral_w as usize,
            NUM,
            100.0 * (integral_w / (NUM as f64))
        );

        let integral_r = est(t, 0.95);
        println!(
            ".. {}/{} articles read ({:.1}%)",
            integral_r as usize,
            NUM,
            100.0 * (integral_r / (NUM as f64))
        );
    }

    for &pt in &[0.9, 0.95, 0.99] {
        let mut p = 0.0;
        let mut k = 1;
        let harmonic = harmonic(NUM, EXP);
        while p < pt && k <= NUM {
            p += zipf(k, EXP, harmonic);
            k += 1;
        }
        println!(
            "the first {} articles ({:.1}%) make up {:.1}% of requests",
            k,
            100.0 * k as f64 / NUM as f64,
            100.0 * p
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

fn est(t: usize, fraction: f64) -> f64 {
    let harmonic = harmonic(NUM, EXP);
    (1..=NUM)
        .map(|k| 1.0 - (1.0 - zipf(k, EXP, harmonic)).powf((t * RATE) as f64 * fraction))
        .sum()
}
