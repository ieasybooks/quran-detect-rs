//! Parsing, argument validation, completions and the spec endpoint.

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use quran_detect::{Brackets, Config, ErrorKinds, ExactContext, Formatting, Preset, Script};
use usage_rs::ValidationError;
use usage_rs::embedded::{Exit, Outcome};
use usage_rs::install::{Env, Loading, OnForeign, Wrote};

use crate::cli::{
    BracketsArg, Cli, Command, Completions, Io, Matching, PresetArg, Restoring, ScriptArg,
};

/// Expand argument files and let Usage render help and validation errors.
pub(crate) fn parse() -> Result<Command, ExitCode> {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    // Shell completion words are unfinished input; never expand their @files.
    if let Some(answer) = Cli::completion_request(&argv) {
        return Err(print_exit(&Exit {
            text: answer,
            stderr: false,
            code: 0,
        }));
    }
    let mut argv = usage_rs::response::expand(argv).map_err(|error| {
        print_exit(&Exit {
            text: format!("error: {error}\n"),
            stderr: true,
            code: 2,
        })
    })?;
    // Usage 7.1.2 treats a leading lone dash as a subcommand name. Route the
    // conventional stdin shorthand explicitly; completion input is untouched.
    if argv.first().is_some_and(|word| word == "-") {
        argv.insert(0, OsString::from("replace"));
    }
    let argv: Vec<_> = argv.iter().map(OsString::as_os_str).collect();
    if Cli::spec_request(&argv).is_some() {
        return Ok(Command::Spec);
    }
    match usage_rs::embedded::outcome(Cli::spec(), Cli::command(), &argv, parse_from) {
        Outcome::Parsed(command) => Ok(command),
        Outcome::Exit(exit) => Err(print_exit(&exit)),
    }
}

fn parse_from<'v>(argv: &[&'v OsStr]) -> Result<Command, usage_rs::Error<'static, 'v>> {
    let cli = Cli::parse_from(argv)?;
    validate(&cli.command).map_err(ValidationError::into_parse_error)?;
    Ok(cli.command)
}

fn validate(command: &Command) -> Result<(), ValidationError> {
    match command {
        Command::Replace(args) => {
            validate_io(&args.io)?;
            config(&args.matching)?;
            validate_path("--matches", args.restoring.matches.as_deref())
        }
        Command::Plan(args) => {
            validate_io(&args.io)?;
            config(&args.matching)?;
            validate_path("--matches", args.restoring.matches.as_deref())
        }
        Command::Identify(args) => {
            validate_io(&args.io)?;
            config(&args.matching).map(|_| ())
        }
        Command::Apply(args) | Command::Revert(args) => {
            validate_io(&args.io)?;
            validate_path("--plan", Some(&args.plan))
        }
        Command::Completions(_) | Command::Spec => Ok(()),
    }
}

fn validate_io(io: &Io) -> Result<(), ValidationError> {
    if io.input.is_some() == io.text.is_some() {
        return Err(ValidationError::field("INPUT").reason("supply exactly one INPUT or --text"));
    }
    validate_path("INPUT", io.input.as_deref())?;
    validate_path("--output", io.output.as_deref())
}

fn validate_path(flag: &'static str, path: Option<&Path>) -> Result<(), ValidationError> {
    if path.is_some_and(|path| path.as_os_str().is_empty()) {
        return Err(ValidationError::field(flag).reason("must not be an empty path"));
    }
    Ok(())
}

/// The library configuration for the matching flags, validated.
pub(crate) fn config(matching: &Matching) -> Result<Config, ValidationError> {
    let preset = match matching.preset {
        PresetArg::Strict => Preset::Strict,
        PresetArg::Balanced => Preset::Balanced,
        PresetArg::Tolerant => Preset::Tolerant,
    };
    let mut config = Config::preset(preset);
    if let Some(fraction) = matching.error_fraction {
        config = config.with_error_fraction(fraction).map_err(|_| {
            ValidationError::field("--error-fraction")
                .value(fraction.to_string())
                .reason("must be a finite number from 0 to 1")
        })?;
    }
    if let Some(edits) = matching.max_edits {
        config = config.with_max_edits(edits);
    }
    if let Some(words) = matching.min_words {
        config = config.with_min_words(words).map_err(|_| {
            ValidationError::field("--min-words")
                .value(words.to_string())
                .reason("must be at least 2")
        })?;
    }
    if let Some(list) = &matching.allowed_errors {
        let kinds = ErrorKinds::parse_list(list).map_err(|error| {
            ValidationError::field("--allowed-errors")
                .value(list.clone())
                .reason(error.to_string())
        })?;
        config = config.with_allowed_errors(kinds);
    }
    if let Some(context) = &matching.min_exact_context {
        let context = ExactContext::parse(context).map_err(|_| {
            ValidationError::field("--min-exact-context")
                .value(context.clone())
                .reason("must be a nonnegative integer or a BEFORE,AFTER pair")
        })?;
        config = config.with_min_exact_context(context);
    }
    Ok(config)
}

pub(crate) fn script(matching: &Matching) -> Script {
    match matching.script {
        ScriptArg::SimplePlain => Script::SimplePlain,
        ScriptArg::Simple => Script::Simple,
    }
}

pub(crate) fn formatting(restoring: &Restoring) -> Formatting {
    Formatting {
        append_reference: restoring.append_reference,
        brackets: match restoring.brackets {
            BracketsArg::Keep => Brackets::Keep,
            BracketsArg::Braces => Brackets::Braces,
            BracketsArg::BracesTrimmed => Brackets::BracesTrimmed,
        },
    }
}

fn print_exit(exit: &Exit) -> ExitCode {
    let result = if exit.stderr {
        io::stderr().lock().write_all(exit.text.as_bytes())
    } else {
        io::stdout().lock().write_all(exit.text.as_bytes())
    };
    match result {
        Ok(()) => ExitCode::from(u8::try_from(exit.code).unwrap_or(1)),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: writing CLI output: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Write a script to stdout or safely install it, optionally previewing its destination.
pub(crate) fn completions(args: &Completions) -> anyhow::Result<()> {
    let shell = usage_rs::complete::Shell::from_name(&args.shell)
        .expect("shell choices validated by Usage");
    if !args.install {
        io::stdout()
            .lock()
            .write_all(Cli::completion_script(shell).as_bytes())?;
        return Ok(());
    }
    let env = Env::from_process();
    let plan = if args.dry_run {
        let plan = Cli::completion_install_plan(shell, &env)?;
        println!("Would install completions to {}", plan.path.display());
        plan
    } else {
        let installed = Cli::install_completion(shell, &env, OnForeign::Refuse)?;
        let action = match installed.wrote {
            Wrote::Updated => "Updated",
            Wrote::Unchanged => "Already installed",
            _ => "Installed",
        };
        println!("{action}: {}", installed.plan.path.display());
        installed.plan
    };
    if let Loading::Manual { line, file, .. } = &plan.loading {
        println!("Add this to {file}:\n{line}");
    }
    if let Some(note) = plan.note {
        println!("Note: {note}");
    }
    Ok(())
}

/// Print the portable CLI definition.
pub(crate) fn spec() -> anyhow::Result<()> {
    io::stdout().lock().write_all(Cli::to_kdl().as_bytes())?;
    Ok(())
}
