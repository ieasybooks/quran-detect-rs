//! Command-line declarations, shared by help, completions and generated documentation.
#![allow(
    clippy::doc_markdown,
    reason = "doc comments are --help text, where backticks would show literally"
)]

use std::path::PathBuf;

use usage_rs::ValueEnum;

const HELP_FOOTER: &str = "\
Offsets in JSON output are UTF-8 byte offsets, as half-open [start, end) ranges.
Text outside detected quotations is preserved byte for byte, including line endings.
Matching recognizes canonical wording; it does not establish that a writer meant to quote.

Use @FILE for shell-style argument files; @@ escapes a literal @.";

/// Detect Quran quotations in Arabic text and restore their canonical wording.
///
/// Replace is the default command. Use identify to list quotations as JSON, plan to
/// record every edit for an audit trail, and apply or revert to replay or undo a plan.
/// Text comes from the embedded Tanzil Quran (Hafs, Kufan verse count).
#[derive(Debug, Clone, usage_rs::Cli)]
#[usage(
    bin = "quran-detect",
    version,
    repository = env!("CARGO_PKG_REPOSITORY"),
    license = env!("CARGO_PKG_LICENSE"),
    completion,
    default_subcommand = "replace",
    default_subcommand_flags,
    default_subcommand_help,
    arg_required_else_help,
    unknown_flags = "error",
    max_term_width = 100,
    exit_code(0, "Success"),
    exit_code(1, "Input, output, match or plan error"),
    exit_code(2, "Invalid arguments"),
    example("quran-detect book.txt -o book.restored.txt", help = "Restore quotations in a text file"),
    example("quran-detect identify --preset strict book.txt", help = "List exact quotations as JSON"),
    example("quran-detect completions zsh --install", help = "Install shell completions")
)]
pub(crate) struct Cli {
    #[usage(subcommand)]
    pub(crate) command: Command,
}

/// Operations available from the command line.
#[derive(Debug, Clone, usage_rs::Subcommands)]
pub(crate) enum Command {
    /// Restore detected quotations to their canonical wording.
    Replace(Box<Replace>),
    /// List detected quotations as JSON.
    Identify(Box<Identify>),
    /// Write a JSON plan of every edit without applying it.
    Plan(Box<PlanCommand>),
    /// Apply a saved plan to its exact original input.
    Apply(Box<PlanFile>),
    /// Undo a saved plan, restoring the exact original input.
    Revert(Box<PlanFile>),
    /// Generate or install shell completions.
    Completions(Completions),
    /// Print the Usage specification as KDL for docs, man pages and tooling.
    Spec,
}

/// Where text comes from and where the result goes.
#[derive(Debug, Clone, usage_rs::Args)]
pub(crate) struct Io {
    /// A UTF-8 text file, or - for stdin.
    #[usage(arg, name = "INPUT", value_hint = usage_rs::ValueHint::FilePath)]
    pub(crate) input: Option<PathBuf>,

    /// Direct text instead of INPUT; whitespace is kept exactly.
    #[usage(long, value_name = "TEXT")]
    pub(crate) text: Option<String>,

    /// Destination file, or - for stdout [default: stdout].
    #[usage(short = 'o', long, value_name = "PATH", value_hint = usage_rs::ValueHint::FilePath)]
    pub(crate) output: Option<PathBuf>,

    /// Replace an existing output file atomically, including INPUT if it is the destination.
    #[usage(long)]
    pub(crate) force: bool,
}

/// How aggressively quotations are matched.
#[derive(Debug, Clone, usage_rs::Args)]
pub(crate) struct Matching {
    /// Aggressiveness preset; the options below override its individual values.
    #[usage(long, value_enum, value_name = "PRESET", default = "balanced")]
    pub(crate) preset: PresetArg,

    /// Maximum alignment errors as a fraction of the passage length, from 0 to 1.
    #[usage(long, value_name = "F")]
    pub(crate) error_fraction: Option<f64>,

    /// Maximum alignment errors in one quotation.
    #[usage(long, value_name = "N")]
    pub(crate) max_edits: Option<u32>,

    /// Minimum canonical words in a quotation; at least 2.
    #[usage(long, value_name = "N")]
    pub(crate) min_words: Option<u32>,

    /// Comma-separated errors to allow: typo, missing, extra, join, split, substitution.
    /// An empty value allows none.
    #[usage(long, value_name = "LIST")]
    pub(crate) allowed_errors: Option<String>,

    /// Exact words required around missing, extra or substituted words: N or BEFORE,AFTER.
    /// 0 disables the check.
    #[usage(long, value_name = "WORDS")]
    pub(crate) min_exact_context: Option<String>,

    /// Spelling written into restored quotations; detection is the same for both.
    #[usage(long, value_enum, value_name = "SCRIPT", default = "simple-plain")]
    pub(crate) script: ScriptArg,
}

/// Optional edits around restored quotations, and explicit match selection.
#[derive(Debug, Clone, usage_rs::Args)]
pub(crate) struct Restoring {
    /// Append a citation such as [البقرة - ٢٥٥] after each quotation.
    #[usage(long)]
    pub(crate) append_reference: bool,

    /// Brackets directly enclosing a quotation.
    #[usage(long, value_enum, value_name = "MODE", default = "keep")]
    pub(crate) brackets: BracketsArg,

    /// Apply exactly the matches in this JSON file, as written by identify, instead of
    /// detecting. Keep any compatible subset in order; [] leaves the text unchanged.
    #[usage(long, value_name = "FILE", value_hint = usage_rs::ValueHint::FilePath)]
    pub(crate) matches: Option<PathBuf>,
}

/// Restore detected quotations to their canonical wording.
///
/// Supply INPUT or --text; a single dash reads UTF-8 text from stdin. The restored text
/// goes to stdout by default, with no added newline.
#[derive(Debug, Clone, usage_rs::Args)]
#[usage(
    unknown_flags = "error",
    max_term_width = 100,
    after_help = HELP_FOOTER,
    example("quran-detect book.txt -o book.restored.txt", help = "Restore a text file"),
    example("quran-detect --text 'قل هو الله احد' --append-reference", help = "Restore and cite"),
    example("quran-detect book.txt --matches chosen.json", help = "Apply only selected matches")
)]
pub(crate) struct Replace {
    #[usage(flatten)]
    pub(crate) io: Io,
    #[usage(flatten, next_help_heading = "Matching")]
    pub(crate) matching: Matching,
    #[usage(flatten, next_help_heading = "Restoring")]
    pub(crate) restoring: Restoring,
}

/// List detected quotations as a JSON array.
///
/// Each match has its byte span, canonical text, verse references, edits and identity.
/// Edit the array and pass it to replace or plan with --matches to apply a selection.
#[derive(Debug, Clone, usage_rs::Args)]
#[usage(
    unknown_flags = "error",
    max_term_width = 100,
    after_help = HELP_FOOTER,
    example("quran-detect identify book.txt", help = "List recommended quotations"),
    example("quran-detect identify --alternatives book.txt -o candidates.json", help = "Include overlapping alternatives")
)]
pub(crate) struct Identify {
    #[usage(flatten)]
    pub(crate) io: Io,
    #[usage(flatten, next_help_heading = "Matching")]
    pub(crate) matching: Matching,

    /// Include every overlapping candidate, each marked recommended or not.
    #[usage(long)]
    pub(crate) alternatives: bool,
}

/// Write a JSON plan of every edit without applying it.
///
/// The plan records hashes of the input and output, so apply and revert only accept
/// the exact texts it was made for.
#[derive(Debug, Clone, usage_rs::Args)]
#[usage(
    unknown_flags = "error",
    max_term_width = 100,
    after_help = HELP_FOOTER,
    example("quran-detect plan book.txt -o plan.json", help = "Record the edits")
)]
pub(crate) struct PlanCommand {
    #[usage(flatten)]
    pub(crate) io: Io,
    #[usage(flatten, next_help_heading = "Matching")]
    pub(crate) matching: Matching,
    #[usage(flatten, next_help_heading = "Restoring")]
    pub(crate) restoring: Restoring,
}

/// Apply or undo a saved plan.
///
/// The input must be the exact text the plan expects: its original input for apply, or
/// its output for revert. Offsets are never rebased onto changed text.
#[derive(Debug, Clone, usage_rs::Args)]
#[usage(
    unknown_flags = "error",
    max_term_width = 100,
    example(
        "quran-detect apply book.txt --plan plan.json -o restored.txt",
        help = "Apply a plan"
    ),
    example(
        "quran-detect revert restored.txt --plan plan.json -o original.txt",
        help = "Undo it"
    )
)]
pub(crate) struct PlanFile {
    #[usage(flatten)]
    pub(crate) io: Io,

    /// A plan written by quran-detect plan.
    #[usage(long, value_name = "FILE", value_hint = usage_rs::ValueHint::FilePath)]
    pub(crate) plan: PathBuf,
}

/// Generate a completion script, or install it in the shell's user directory.
///
/// Scripts call quran-detect directly; no separate usage executable is needed.
#[derive(Debug, Clone, usage_rs::Args)]
#[usage(
    unknown_flags = "error",
    example("quran-detect completions bash", help = "Print a script to stdout"),
    example(
        "quran-detect completions zsh --install --dry-run",
        help = "Preview installation"
    )
)]
pub(crate) struct Completions {
    /// Shell to generate completions for.
    #[usage(arg, choices("bash", "elvish", "fish", "nu", "powershell", "zsh"))]
    pub(crate) shell: String,

    /// Install into this shell's user completion directory.
    #[usage(long)]
    pub(crate) install: bool,

    /// Show the destination and setup instructions without writing a file.
    #[usage(long, requires("--install"))]
    pub(crate) dry_run: bool,
}

/// Value of --preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum PresetArg {
    /// Exact normalized matches only.
    Strict,
    /// Typo, missing, extra, join and split errors within 25%.
    Balanced,
    /// Balanced errors plus substitution within 35%.
    Tolerant,
}

/// Value of --script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ScriptArg {
    /// Tanzil Simple Plain with pause marks.
    SimplePlain,
    /// Tanzil Simple with pause marks and ikhfa/idgham marks.
    Simple,
}

/// Value of --brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum BracketsArg {
    /// Leave brackets unchanged.
    Keep,
    /// Convert «», “”, (), [], ﴿﴾ or {} around a quotation to {}.
    Braces,
    /// Convert to {} and remove whitespace just inside them.
    BracesTrimmed,
}
