//! Reading UTF-8 input and publishing complete output files atomically.

use std::fs::{self, Permissions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use serde::de::DeserializeOwned;
use tempfile::NamedTempFile;

use crate::cli::Io;

/// The exact input text, without normalization or newline conversion.
pub(crate) fn read_text(io: &Io) -> anyhow::Result<String> {
    if let Some(text) = &io.text {
        return Ok(text.clone());
    }
    let path = io.input.as_deref().expect("INPUT validated");
    if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut bytes)
            .context("reading stdin")?;
        return String::from_utf8(bytes).context("stdin is not valid UTF-8");
    }
    let bytes = read_file(path, "input")?;
    String::from_utf8(bytes).with_context(|| format!("input {} is not valid UTF-8", path.display()))
}

/// A JSON document such as a plan or a match list.
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path, what: &str) -> anyhow::Result<T> {
    let bytes = read_file(path, what)?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("reading {what} {} as JSON", path.display()))
}

fn read_file(path: &Path, what: &str) -> anyhow::Result<Vec<u8>> {
    let metadata =
        fs::metadata(path).with_context(|| format!("reading {what} {}", path.display()))?;
    ensure!(
        metadata.is_file(),
        "{what} {} is not a regular file",
        path.display()
    );
    fs::read(path).with_context(|| format!("reading {what} {}", path.display()))
}

/// A destination checked before work starts, without creating or truncating anything.
pub(crate) enum Output {
    Stdout,
    File {
        path: PathBuf,
        force: bool,
        permissions: Option<Permissions>,
    },
}

impl Output {
    /// Refuse an existing destination unless forced, and never replace a non-file.
    pub(crate) fn prepare(path: Option<&Path>, force: bool) -> anyhow::Result<Self> {
        let Some(path) = path.filter(|path| *path != Path::new("-")) else {
            return Ok(Self::Stdout);
        };
        let permissions = match fs::symlink_metadata(path) {
            Ok(metadata) => {
                ensure!(
                    force,
                    "output {} already exists; use --force to replace it",
                    path.display()
                );
                ensure!(
                    metadata.is_file(),
                    "output {} is not a regular file; refusing to replace it",
                    path.display()
                );
                Some(metadata.permissions())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("checking output {}", path.display()));
            }
        };
        Ok(Self::File {
            path: path.to_owned(),
            force,
            permissions,
        })
    }

    /// Write the complete result; a file appears only once fully written and flushed.
    pub(crate) fn write(self, bytes: &[u8]) -> anyhow::Result<()> {
        match self {
            Self::Stdout => {
                let mut stdout = io::stdout().lock();
                stdout
                    .write_all(bytes)
                    .context("writing result to stdout")?;
                stdout.flush().context("flushing stdout")?;
            }
            Self::File {
                path,
                force,
                permissions,
            } => {
                let parent = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let mut temporary = NamedTempFile::new_in(parent).with_context(|| {
                    format!(
                        "preparing output {} (its parent directory must exist and be writable)",
                        path.display()
                    )
                })?;
                temporary
                    .write_all(bytes)
                    .with_context(|| format!("writing temporary output for {}", path.display()))?;
                if let Some(permissions) = permissions {
                    temporary.as_file().set_permissions(permissions)?;
                }
                temporary
                    .as_file()
                    .sync_all()
                    .with_context(|| format!("flushing temporary output for {}", path.display()))?;
                let result = if force {
                    temporary.persist(&path)
                } else {
                    temporary.persist_noclobber(&path)
                };
                if let Err(error) = result {
                    if !force && error.error.kind() == io::ErrorKind::AlreadyExists {
                        bail!(
                            "output {} already exists; use --force to replace it",
                            path.display()
                        );
                    }
                    return Err(error.error)
                        .with_context(|| format!("publishing output {}", path.display()));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::Output;

    #[test]
    fn a_destination_created_during_work_is_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("result.txt");
        let output = Output::prepare(Some(&path), false).unwrap();
        fs::write(&path, "written by someone else").unwrap();
        let error = output.write(b"result").unwrap_err();
        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "written by someone else"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn only_force_replaces_a_file_and_publishes_exact_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("result.txt");
        fs::write(&path, "original").unwrap();
        assert!(Output::prepare(Some(&path), false).is_err());
        let output = Output::prepare(Some(&path), true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        let result = "\u{feff}\tقُلْ هُوَ اللَّهُ أَحَدٌ\r\n  ";
        output.write(result.as_bytes()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), result.as_bytes());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn even_dangling_symlinks_are_protected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("result.txt");
        std::os::unix::fs::symlink(directory.path().join("missing.txt"), &path).unwrap();
        assert!(Output::prepare(Some(&path), false).is_err());
        assert!(Output::prepare(Some(&path), true).is_err());
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
