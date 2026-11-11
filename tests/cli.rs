//! Command-line behavior: commands, validation, exact bytes and safe output handling.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn command(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_quran-detect"));
    command
        .current_dir(directory)
        .env("HOME", directory)
        .env("USERPROFILE", directory)
        .env("XDG_CONFIG_HOME", directory.join("config"))
        .env("XDG_DATA_HOME", directory.join("data"))
        .env("APPDATA", directory.join("AppData/Roaming"))
        .env("LOCALAPPDATA", directory.join("AppData/Local"))
        .env_remove("CLICOLOR_FORCE")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null());
    command
}

fn run(directory: &Path, args: &[&str]) -> Output {
    command(directory).args(args).output().unwrap()
}

fn run_stdin(directory: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command(directory)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn success(output: &Output) {
    assert!(output.status.success(), "{}", stderr(output));
    assert!(output.stderr.is_empty(), "{}", stderr(output));
}

fn failure(output: &Output, code: i32, message: &str) {
    assert_eq!(output.status.code(), Some(code), "{}", stderr(output));
    assert!(output.stdout.is_empty(), "{}", stdout(output));
    assert!(
        stderr(output).contains(message),
        "expected {message:?}: {}",
        stderr(output)
    );
}

fn json(output: &Output) -> Value {
    success(output);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn help_version_and_spec_need_no_input() {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec!["--help"],
        vec!["replace", "--help"],
        vec!["identify", "--help"],
        vec!["plan", "--help"],
        vec!["apply", "--help"],
        vec!["revert", "--help"],
        vec!["--version"],
        vec!["spec"],
        vec!["__usage_spec__"],
    ] {
        let output = run(directory.path(), &args);
        success(&output);
        assert!(!output.stdout.is_empty(), "{args:?}");
    }
    let help = stdout(&run(directory.path(), &["replace", "--help"]));
    for text in [
        "--preset",
        "--error-fraction",
        "--max-edits",
        "--min-words",
        "--allowed-errors",
        "--min-exact-context",
        "--script",
        "--append-reference",
        "--brackets",
        "--matches",
        "--force",
        "@FILE",
    ] {
        assert!(help.contains(text), "missing {text}: {help}");
    }
    let spec = stdout(&run(directory.path(), &["spec"]));
    assert_eq!(spec, stdout(&run(directory.path(), &["__usage_spec__"])));
    for word in ["simple-plain", "braces-trimmed", "tolerant"] {
        assert!(spec.contains(word), "{word}");
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn invalid_arguments_exit_two_without_output() {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec![],
        vec!["replace"],
        vec!["identify"],
        vec!["--text", "قل", "input.txt"],
        vec!["first.txt", "second.txt"],
        vec!["--text", "قل", "--preset", "loose"],
        vec!["--text", "قل", "--script", "uthmani"],
        vec!["--text", "قل", "--brackets", "round"],
        vec!["--text", "قل", "--error-fraction", "1.5"],
        vec!["--text", "قل", "--error-fraction", "NaN"],
        vec!["--text", "قل", "--error-fraction", "x"],
        vec!["--text", "قل", "--max-edits", "-1"],
        vec!["--text", "قل", "--min-words", "1"],
        vec!["--text", "قل", "--allowed-errors", "typo,guess"],
        vec!["--text", "قل", "--min-exact-context", "1,2,3"],
        vec!["--text", "قل", "--min-exact-context", "-1"],
        vec!["--text", "قل", "--output", ""],
        vec!["--text", "قل", "--matches", ""],
        vec!["identify", "--text", "قل", "--append-reference"],
        vec!["identify", "--text", "قل", "--matches", "m.json"],
        vec!["apply", "--text", "قل"],
        vec!["apply", "--text", "قل", "--plan", ""],
        vec![
            "revert", "--text", "قل", "--preset", "strict", "--plan", "p.json",
        ],
        vec!["--text", "قل", "--typo"],
    ] {
        let output = run(directory.path(), &args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{args:?}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "{args:?}: {}", stdout(&output));
        assert!(
            stderr(&output).contains("error:") || stderr(&output).contains("Usage:"),
            "{args:?}"
        );
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn replace_preserves_exact_bytes_and_protects_existing_files() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("in.txt");
    let destination = directory.path().join("out.txt");
    fs::write(&source, "🙂\r\nفيختم بقل هو الله أحد\r\n").unwrap();
    success(&run(directory.path(), &["in.txt", "-o", "out.txt"]));
    assert_eq!(
        fs::read(&destination).unwrap(),
        "🙂\r\nفيختم بقُلْ هُوَ اللَّهُ أَحَدٌ\r\n".as_bytes()
    );
    fs::write(&destination, "keep").unwrap();
    failure(
        &run(directory.path(), &["in.txt", "-o", "out.txt"]),
        1,
        "--force",
    );
    assert_eq!(fs::read_to_string(&destination).unwrap(), "keep");
    success(&run(
        directory.path(),
        &["in.txt", "-o", "out.txt", "--force"],
    ));
    assert!(fs::read_to_string(&destination).unwrap().contains("بقُلْ"));
    // Restoring in place is allowed only with --force, and is atomic.
    failure(
        &run(directory.path(), &["in.txt", "-o", "in.txt"]),
        1,
        "--force",
    );
    success(&run(
        directory.path(),
        &["in.txt", "-o", "in.txt", "--force"],
    ));
    assert_eq!(fs::read(&source).unwrap(), fs::read(&destination).unwrap());
    let stdout_text = run(directory.path(), &["--text", " قل هو الله أحد\n"]);
    success(&stdout_text);
    assert_eq!(stdout(&stdout_text), " قُلْ هُوَ اللَّهُ أَحَدٌ\n");
    assert_eq!(stdout(&run(directory.path(), &["--text", ""])), "");
    let names: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn stdin_and_input_errors() {
    let directory = tempfile::tempdir().unwrap();
    let output = run_stdin(directory.path(), &["-"], "فيختم بقل هو الله أحد".as_bytes());
    success(&output);
    assert_eq!(stdout(&output), "فيختم بقُلْ هُوَ اللَّهُ أَحَدٌ");
    let output = run_stdin(
        directory.path(),
        &["identify", "-"],
        "قل هو الله أحد".as_bytes(),
    );
    assert_eq!(json(&output).as_array().unwrap().len(), 1);
    failure(
        &run_stdin(directory.path(), &["-"], b"\xff\xfe"),
        1,
        "not valid UTF-8",
    );
    failure(&run(directory.path(), &["missing.txt"]), 1, "missing.txt");
    fs::create_dir(directory.path().join("folder")).unwrap();
    failure(&run(directory.path(), &["folder"]), 1, "not a regular file");
    failure(
        &run(directory.path(), &["--text", "قل", "-o", "absent/out.txt"]),
        1,
        "parent directory",
    );
}

#[test]
fn identify_reports_byte_offsets_and_alternatives() {
    let directory = tempfile::tempdir().unwrap();
    let text = "🙂 قال: الله لا إله إلا هو الحي القيوم، ثم قل هو الله أحد";
    let matches = json(&run(directory.path(), &["identify", "--text", text]));
    let matches = matches.as_array().unwrap();
    assert_eq!(matches.len(), 2);
    for m in matches {
        let (start, end) = (m["start"].as_u64().unwrap(), m["end"].as_u64().unwrap());
        let span = &text[usize::try_from(start).unwrap()..usize::try_from(end).unwrap()];
        assert_eq!(span, m["text"].as_str().unwrap());
        assert_eq!(m["edition"]["riwaya"], "hafs");
        assert_eq!(m["edition"]["counting"], "kufi");
        assert_eq!(m["edition"]["script"], "simple-plain");
        assert_eq!(m["recommended"], true);
    }
    let all = json(&run(
        directory.path(),
        &["identify", "--alternatives", "--text", text],
    ));
    let all = all.as_array().unwrap();
    assert!(all.len() > matches.len());
    let recommended: Vec<&Value> = all.iter().filter(|m| m["recommended"] == true).collect();
    assert_eq!(recommended, matches.iter().collect::<Vec<_>>());
    let strict = json(&run(
        directory.path(),
        &[
            "identify", "--preset", "strict", "--script", "simple", "--text", text,
        ],
    ));
    assert_eq!(strict[0]["edition"]["script"], "simple");
}

#[test]
fn matches_read_back_from_json_keep_exact_float_values() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("in.txt"), "قل هو الله أحد").unwrap();
    let mut matches = json(&run(directory.path(), &["identify", "in.txt"]));
    // A value that serde_json parses one unit off unless parsing is correctly rounded.
    let exact = "0.15384615384615385";
    matches[0]["error_fraction"] = serde_json::from_str(exact).unwrap();
    let file = directory.path().join("matches.json");
    fs::write(&file, serde_json::to_vec(&matches).unwrap()).unwrap();
    assert!(fs::read_to_string(&file).unwrap().contains(exact));
    let plan = run(
        directory.path(),
        &["plan", "in.txt", "--matches", "matches.json"],
    );
    success(&plan);
    assert!(stdout(&plan).contains(&format!("\"error_fraction\": {exact}")));
}

#[test]
fn plan_apply_and_revert_round_trip_through_files() {
    let directory = tempfile::tempdir().unwrap();
    let source = "قال: « قل هو الله أحد »\r\nوالسلام";
    fs::write(directory.path().join("in.txt"), source).unwrap();
    success(&run(
        directory.path(),
        &[
            "plan",
            "in.txt",
            "--append-reference",
            "--brackets",
            "braces-trimmed",
            "-o",
            "plan.json",
        ],
    ));
    let plan: Value =
        serde_json::from_slice(&fs::read(directory.path().join("plan.json")).unwrap()).unwrap();
    assert_eq!(plan["version"], "2.0");
    assert_eq!(plan["offset_unit"], "UTF-8 bytes, [start,end)");
    assert_eq!(plan["formatting"]["brackets"], "braces-trimmed");
    success(&run(
        directory.path(),
        &["apply", "in.txt", "--plan", "plan.json", "-o", "out.txt"],
    ));
    let restored = fs::read_to_string(directory.path().join("out.txt")).unwrap();
    assert_eq!(restored, "قال: {قُلْ هُوَ اللَّهُ أَحَدٌ} [الإخلاص - ١]\r\nوالسلام");
    let direct = run(
        directory.path(),
        &[
            "in.txt",
            "--append-reference",
            "--brackets",
            "braces-trimmed",
        ],
    );
    assert_eq!(stdout(&direct), restored);
    success(&run(
        directory.path(),
        &["revert", "out.txt", "--plan", "plan.json", "-o", "back.txt"],
    ));
    assert_eq!(
        fs::read_to_string(directory.path().join("back.txt")).unwrap(),
        source
    );
    failure(
        &run(
            directory.path(),
            &["apply", "out.txt", "--plan", "plan.json"],
        ),
        1,
        "input hash",
    );
    failure(
        &run(
            directory.path(),
            &["revert", "in.txt", "--plan", "plan.json"],
        ),
        1,
        "output hash",
    );
    fs::write(directory.path().join("bad.json"), "{}").unwrap();
    failure(
        &run(directory.path(), &["apply", "in.txt", "--plan", "bad.json"]),
        1,
        "bad.json",
    );
}

#[test]
fn explicit_matches_are_applied_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let text = "«الله لا إله إلا هو الحي القيوم» ثم قل هو الله أحد";
    fs::write(directory.path().join("in.txt"), text).unwrap();
    let all = json(&run(directory.path(), &["identify", "in.txt"]));
    let mut first = all[0].clone();
    let choice = first["reference_alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["references"] == serde_json::json!([[3, 2]]))
        .unwrap()["id"]
        .clone();
    first["reference_choice"] = choice;
    fs::write(
        directory.path().join("one.json"),
        serde_json::to_vec(&[first]).unwrap(),
    )
    .unwrap();
    let output = run(
        directory.path(),
        &[
            "in.txt",
            "--matches",
            "one.json",
            "--append-reference",
            "--brackets",
            "braces",
        ],
    );
    success(&output);
    let restored = stdout(&output);
    assert!(
        restored.contains("} [آل عمران - ٢] ثم قل هو الله أحد"),
        "{restored}"
    );
    fs::write(directory.path().join("none.json"), "[]").unwrap();
    assert_eq!(
        stdout(&run(
            directory.path(),
            &["in.txt", "--matches", "none.json"]
        )),
        text
    );
    let reversed: Vec<Value> = all.as_array().unwrap().iter().rev().cloned().collect();
    fs::write(
        directory.path().join("reversed.json"),
        serde_json::to_vec(&reversed).unwrap(),
    )
    .unwrap();
    failure(
        &run(directory.path(), &["in.txt", "--matches", "reversed.json"]),
        1,
        "ordered, nonoverlapping",
    );
    failure(
        &run(
            directory.path(),
            &["in.txt", "--matches", "one.json", "--script", "simple"],
        ),
        1,
        "hafs/kufi/simple-plain",
    );
    failure(
        &run(
            directory.path(),
            &["--text", "x", "--matches", "missing.json"],
        ),
        1,
        "missing.json",
    );
}

#[test]
fn context_pairs_and_formatting_flags_match_the_reference_cli() {
    let directory = tempfile::tempdir().unwrap();
    let source = "«قل هو الله أحد»".as_bytes();
    let output = run_stdin(
        directory.path(),
        &[
            "identify",
            "-",
            "--alternatives",
            "--min-exact-context",
            "1,2",
        ],
        source,
    );
    assert_eq!(json(&output)[0]["recommended"], true);
    let output = run_stdin(
        directory.path(),
        &[
            "-",
            "--min-exact-context",
            "1,2",
            "--append-reference",
            "--brackets",
            "braces-trimmed",
        ],
        source,
    );
    success(&output);
    assert!(stdout(&output).ends_with("} [الإخلاص - ١]"));
    // Expected outputs come from the Python reference with the same corpus.
    let text = "الله لا إله إلا الحي القيوم";
    let repaired = run(directory.path(), &["--text", text]);
    assert_eq!(stdout(&repaired), "اللَّهُ لَا إِلَٰهَ إِلَّا هُوَ الْحَيُّ الْقَيُّومُ");
    let exact_only = run(directory.path(), &["--text", text, "--allowed-errors", ""]);
    assert_eq!(stdout(&exact_only), "اللَّهُ لَا إِلَٰهَ إِلَّا الحي القيوم");
}

#[test]
fn completion_scripts_and_installation() {
    let directory = tempfile::tempdir().unwrap();
    for shell in ["bash", "elvish", "fish", "nu", "powershell", "zsh"] {
        let output = run(directory.path(), &["completions", shell]);
        success(&output);
        assert!(stdout(&output).contains("__complete_word__"), "{shell}");
    }
    for args in [
        vec!["completions", "invalid"],
        vec!["completions", "fish", "--dry-run"],
    ] {
        failure(&run(directory.path(), &args), 2, "error:");
    }
    let result = run(
        directory.path(),
        &[
            "__complete_word__",
            "--shell",
            "bash",
            "--words",
            "quran-detect",
            "--preset",
            "to",
        ],
    );
    success(&result);
    assert!(stdout(&result).contains("tolerant"));
    let target = directory
        .path()
        .join("config/fish/completions/quran-detect.fish");
    let preview = run(
        directory.path(),
        &["completions", "fish", "--install", "--dry-run"],
    );
    success(&preview);
    assert!(stdout(&preview).contains("Would install completions to"));
    assert!(!target.exists());
    success(&run(
        directory.path(),
        &["completions", "fish", "--install"],
    ));
    assert!(
        fs::read_to_string(&target)
            .unwrap()
            .contains("__complete_word__")
    );
    let repeated = run(directory.path(), &["completions", "fish", "--install"]);
    success(&repeated);
    assert!(stdout(&repeated).contains("Already installed"));
}

#[test]
fn response_files_and_literal_at_paths() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("cite.args"),
        "--append-reference --brackets braces --text '«قل هو الله أحد»'",
    )
    .unwrap();
    let output = run(directory.path(), &["@cite.args"]);
    success(&output);
    assert_eq!(stdout(&output), "{قُلْ هُوَ اللَّهُ أَحَدٌ} [الإخلاص - ١]");
    fs::write(directory.path().join("cycle.args"), "@cycle.args").unwrap();
    fs::write(directory.path().join("broken.args"), "'unclosed").unwrap();
    for file in ["cycle.args", "broken.args", "missing.args"] {
        failure(&run(directory.path(), &[&format!("@{file}")]), 2, file);
    }
    fs::write(directory.path().join("@input.txt"), "قل هو الله أحد").unwrap();
    let output = run(directory.path(), &["@@input.txt"]);
    success(&output);
    assert_eq!(stdout(&output), "قُلْ هُوَ اللَّهُ أَحَدٌ");
}
