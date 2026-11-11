//! Development tool for `scripts/parity.py`, which compares this crate with the Python
//! reference implementation. Reads JSON Lines requests on stdin and writes one JSON
//! result per line on stdout.

use std::io::{self, BufRead, Write};

use quran_detect::{Config, Detector, Formatting, Match, Script};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    text: String,
    config: Config,
    script: Script,
    formattings: Vec<Formatting>,
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    matches: Vec<Match>,
    formatting: Formatting,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for line in io::stdin().lock().lines() {
        let request: Request = serde_json::from_str(&line?)?;
        let detector = Detector::new(request.config).with_script(request.script);
        let text = &request.text;
        let mut plans = Vec::new();
        let mut outputs = Vec::new();
        let mut reverts = Vec::new();
        for &formatting in &request.formattings {
            let plan = detector.plan(text, formatting);
            let output = plan.apply(text)?;
            reverts.push(plan.revert(&output)? == *text);
            assert_eq!(detector.replace(text, formatting), output);
            outputs.push(output);
            plans.push(plan);
        }
        let choices: Vec<Value> = request
            .choices
            .iter()
            .map(
                |choice| match detector.plan_matches(text, &choice.matches, choice.formatting) {
                    Ok(plan) => json!(plan),
                    Err(error) => json!({ "error": error.to_string() }),
                },
            )
            .collect();
        let result = json!({
            "id": request.id,
            "identify": detector.identify(text),
            "alternatives": detector.alternatives(text),
            "plans": plans,
            "outputs": outputs,
            "reverts": reverts,
            "choices": choices,
        });
        serde_json::to_writer(&mut out, &result)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
}
