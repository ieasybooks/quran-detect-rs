//! `quran-detect`: detect Quran quotations in Arabic text and restore their canonical wording.

mod cli;
mod command;
mod io;

use std::process::ExitCode;

use anyhow::Context;
use quran_detect::{Detector, Match, Plan};
use serde::Serialize;

use crate::cli::{Command, Identify, PlanCommand, PlanFile, Replace};
use crate::io::{Output, read_json, read_text};

fn main() -> ExitCode {
    let command = match command::parse() {
        Ok(command) => command,
        Err(code) => return code,
    };
    let result = match command {
        Command::Replace(args) => replace(&args),
        Command::Identify(args) => identify(&args),
        Command::Plan(args) => plan(&args),
        Command::Apply(args) => replay(&args, false),
        Command::Revert(args) => replay(&args, true),
        Command::Completions(args) => command::completions(&args),
        Command::Spec => command::spec(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// A closed stdout, as in `quran-detect identify book.txt | head`, is not a failure.
fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

fn detector(matching: &cli::Matching) -> Detector {
    let config = command::config(matching).expect("matching options validated during parsing");
    Detector::new(config).with_script(command::script(matching))
}

fn json(value: &impl Serialize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).context("serializing JSON")?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn selected_matches(restoring: &cli::Restoring) -> anyhow::Result<Option<Vec<Match>>> {
    restoring
        .matches
        .as_deref()
        .map(|path| read_json(path, "matches"))
        .transpose()
}

fn replace(args: &Replace) -> anyhow::Result<()> {
    let text = read_text(&args.io)?;
    let matches = selected_matches(&args.restoring)?;
    let output = Output::prepare(args.io.output.as_deref(), args.io.force)?;
    let detector = detector(&args.matching);
    let formatting = command::formatting(&args.restoring);
    let restored = match matches {
        Some(matches) => detector
            .replace_matches(&text, &matches, formatting)
            .context("applying --matches")?,
        None => detector.replace(&text, formatting),
    };
    output.write(restored.as_bytes())
}

fn identify(args: &Identify) -> anyhow::Result<()> {
    let text = read_text(&args.io)?;
    let output = Output::prepare(args.io.output.as_deref(), args.io.force)?;
    let detector = detector(&args.matching);
    let matches = if args.alternatives {
        detector.alternatives(&text)
    } else {
        detector.identify(&text)
    };
    output.write(&json(&matches)?)
}

fn plan(args: &PlanCommand) -> anyhow::Result<()> {
    let text = read_text(&args.io)?;
    let matches = selected_matches(&args.restoring)?;
    let output = Output::prepare(args.io.output.as_deref(), args.io.force)?;
    let detector = detector(&args.matching);
    let formatting = command::formatting(&args.restoring);
    let plan = match matches {
        Some(matches) => detector
            .plan_matches(&text, &matches, formatting)
            .context("planning --matches")?,
        None => detector.plan(&text, formatting),
    };
    output.write(&json(&plan)?)
}

fn replay(args: &PlanFile, revert: bool) -> anyhow::Result<()> {
    let text = read_text(&args.io)?;
    let plan: Plan = read_json(&args.plan, "plan")?;
    let output = Output::prepare(args.io.output.as_deref(), args.io.force)?;
    let result = if revert {
        plan.revert(&text).context("reverting the plan")?
    } else {
        plan.apply(&text).context("applying the plan")?
    };
    output.write(result.as_bytes())
}
