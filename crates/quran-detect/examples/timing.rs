//! Development tool for `scripts/timing.py`: time identify and plan in one process.
//!
//! Reads JSON Lines `{"id", "text", "preset"}` on stdin and writes one
//! `{"id", "seconds"}` line per document, after building the corpus index once.

use std::io::{self, BufRead, Write};
use std::time::Instant;

use quran_detect::{Config, Detector, Formatting, Preset};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    text: String,
    preset: Preset,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build the shared corpus index before timing, as a long-lived process would.
    let warmup = Detector::default();
    let started = Instant::now();
    warmup.plan("قل هو الله أحد", Formatting::default());
    eprintln!(
        "corpus index and warmup: {:.3}s",
        started.elapsed().as_secs_f64()
    );
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let detector = Detector::new(Config::preset(request.preset));
        let started = Instant::now();
        let matches = detector.identify(&request.text);
        let plan = detector.plan_matches(&request.text, &matches, Formatting::default())?;
        let output = plan.apply(&request.text)?;
        let seconds = started.elapsed().as_secs_f64();
        std::hint::black_box(output);
        serde_json::to_writer(&mut out, &json!({ "id": request.id, "seconds": seconds }))?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
}
