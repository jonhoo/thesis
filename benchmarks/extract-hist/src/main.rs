use clap::{App, Arg};
use hdrhistogram::serialization::interval_log;
use hdrhistogram::serialization::Deserializer;
use hdrhistogram::Histogram;
use std::collections::HashMap;
use std::time::Duration;
use trawler::LobstersRequest;

fn main() {
    let matches = App::new("Histogram extractor")
        .version("1.0")
        .arg(Arg::with_name("timeline").long("timeline"))
        .arg(
            Arg::with_name("HISTOGRAM")
                .help("Histogram file to analyze")
                .multiple(true)
                .required(true),
        )
        .get_matches();

    let as_timeline = matches.is_present("timeline");
    let filenames = matches.values_of("HISTOGRAM").unwrap();
    let mut deserializer = Deserializer::new();
    let mut timelines = HashMap::<String, Timeline>::default();

    for filename in filenames {
        let file = match std::fs::read(filename) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("failed to read histogram: {}", e);
                return;
            }
        };
        let mut histograms = interval_log::IntervalLogIterator::new(&file);
        let _base_time = match histograms.next().unwrap().unwrap() {
            interval_log::LogEntry::BaseTime(t) => t,
            e => unreachable!("{:?}", e),
        };
        let mut last = histograms.next();

        macro_rules! ex {
            ($name:expr) => {{
                // in this file, there will be a number of histograms for each operation type.
                // specifically, it will be these, in this order:
                //
                //  1. [0-1s) processing
                //  2. [0-1s) sojourn
                //  3. [1-2s) processing
                //  4. [1-2s) sojourn
                //  5. [2-4s) processing
                //  6. [2-4s) sojourn
                //  ...
                //  X. [0-1s) processing <-- for the _next_ operation type!
                //
                // once we have seen a non-0-start histogram, we need to stop parsing histograms
                // the moment we get to the _next_ 0-start histogram (or the end). but we will only
                // realize that that is the case once we've read it. so, we need `last` to "stash
                // away" the tail histogram for use by the next read iteration.
                let mut seen_non_zero = false;
                let mut i = 0;
                while let Some(log_entry) = last.take() {
                    let hist = match log_entry.unwrap() {
                        interval_log::LogEntry::Interval(h) => h,
                        log_entry => {
                            panic!("got unexpected non-interval log entry: {:?}", log_entry);
                        }
                    };

                    if hist.start_timestamp() == Duration::new(0, 0) {
                        if seen_non_zero {
                            // this is the start of the next operation type!
                            last = Some(Ok(interval_log::LogEntry::Interval(hist)));
                            break;
                        }
                    } else {
                        seen_non_zero = true;
                    }

                    let timeline_idx = i / 2;

                    // sanity check
                    let start = Duration::from_secs((1 << timeline_idx) >> 1);
                    assert_eq!(hist.start_timestamp(), start);

                    let mut encoded = hist.encoded_histogram().as_bytes();
                    let mut h = base64::read::DecoderReader::new(&mut encoded, base64::STANDARD);

                    match deserializer.deserialize(&mut h) {
                        Ok(h) => {
                            let tl = timelines.entry($name.to_string()).or_default();
                            tl.last_end = tl.last_end.max(start + hist.duration());
                            if timeline_idx >= tl.histograms.len() {
                                tl.histograms
                                    .resize(timeline_idx + 1, Histograms::default());
                            }
                            let hists = &mut tl.histograms[timeline_idx];
                            match &*hist.tag().expect("untagged histogram") {
                                "sojourn" => hists.sojourn.add(&h).expect("same bounds"),
                                "processing" => hists.processing.add(&h).expect("same bounds"),
                                m => unreachable!("{}", m),
                            }
                        }
                        Err(e) => {
                            eprintln!(concat!("failed to process histograms for {}: {}"), $name, e);
                            break;
                        }
                    }

                    last = histograms.next();
                    i += 1;
                }
            }};
        }

        // operation|"all" => (processing, sojourn)
        if filename.contains("lobsters") {
            // lobsters writes out all the histograms in ::all() order.
            for variant in LobstersRequest::all() {
                ex!(LobstersRequest::variant_name(&variant));
            }
        } else {
            // this is presumably vote. vote writes out write first, then read.
            ex!("writes");
            ex!("reads");
        }
        assert_eq!(last, None, "histogram file had trailing histograms");
    }

    // construct an "all" entry
    let mut values = timelines.values_mut();
    let all = values.next().expect("no histograms?").clone();
    let all = values.fold(all, |mut all, h| {
        all.merge(h);
        all
    });
    timelines.insert("all".to_string(), all);

    if as_timeline {
        println!("op\tuntil\tmetric\tmean\tmedian\tp25\tp90\tp95\tp99\tmax");
        for (op, timeline) in timelines {
            for (i, h) in timeline.histograms.iter().enumerate() {
                let start = Duration::from_secs((1 << i) >> 1);
                let dur = Duration::from_secs(1 << i) - start;
                let end = timeline.last_end.min(start + dur);
                for &metric in &["processing", "sojourn"] {
                    let h = match metric {
                        "processing" => &h.processing,
                        "sojourn" => &h.sojourn,
                        m => unreachable!("{}", m),
                    };
                    if h.max() == 0 {
                        eprintln!("skipping empty histogram: {} {}", metric, op);
                        continue;
                    }
                    println!(
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        op,
                        end.as_secs(),
                        metric,
                        h.mean() / 1000.0, /* use ms */
                        h.value_at_quantile(0.5) as f64 / 1000.0,
                        h.value_at_quantile(0.25) as f64 / 1000.0,
                        h.value_at_quantile(0.90) as f64 / 1000.0,
                        h.value_at_quantile(0.95) as f64 / 1000.0,
                        h.value_at_quantile(0.99) as f64 / 1000.0,
                        h.max() as f64 / 1000.0,
                    );
                }
            }
        }
    } else {
        println!("op\tmetric\tpct\ttime");
        for (op, h) in timelines {
            let h = h.collapse();
            for &metric in &["processing", "sojourn"] {
                let h = match metric {
                    "processing" => &h.processing,
                    "sojourn" => &h.sojourn,
                    m => unreachable!("{}", m),
                };
                if h.max() == 0 {
                    eprintln!("skipping empty histogram: {} {}", metric, op);
                    continue;
                }
                for v in h.iter_quantiles(4) {
                    println!(
                        "{}\t{}\t{}\t{}",
                        op,
                        metric,
                        v.quantile_iterated_to(),
                        v.value_iterated_to() as f64 / 1000.0, /* use ms */
                    );
                }
            }
        }
    }
}

#[derive(Default, Clone)]
pub struct Timeline {
    // these are logarithmically spaced
    // the first histogram is 0-1s after start, the second 1-2s after start, then 2-4s, etc.
    histograms: Vec<Histograms>,
    last_end: std::time::Duration,
}

#[derive(Clone)]
pub struct Histograms {
    processing: Histogram<u64>,
    sojourn: Histogram<u64>,
}

impl Default for Histograms {
    fn default() -> Self {
        Self {
            processing: Histogram::new_with_bounds(1, 60_000_000, 3).unwrap(),
            sojourn: Histogram::new_with_bounds(1, 60_000_000, 3).unwrap(),
        }
    }
}

impl Histograms {
    pub fn merge(&mut self, other: &Self) {
        self.processing.add(&other.processing).expect("same bounds");
        self.sojourn.add(&other.sojourn).expect("same bounds");
    }
}

impl Timeline {
    pub fn merge(&mut self, other: &Self) {
        for (ti, other_hs) in other.histograms.iter().enumerate() {
            if let Some(self_hs) = self.histograms.get_mut(ti) {
                self_hs.merge(other_hs);
            } else {
                self.histograms.push(other_hs.clone());
            }
        }
    }

    pub fn collapse(&self) -> Histograms {
        let mut hists = self.histograms.iter();
        if let Some(hs) = hists.next() {
            let mut proc = hs.processing.clone();
            let mut sjrn = hs.sojourn.clone();
            for hs in hists {
                proc.add(&hs.processing).expect("same bounds");
                sjrn.add(&hs.sojourn).expect("same bounds");
            }
            Histograms {
                processing: proc,
                sojourn: sjrn,
            }
        } else {
            Histograms {
                processing: Histogram::new(1).unwrap(),
                sojourn: Histogram::new(1).unwrap(),
            }
        }
    }
}
