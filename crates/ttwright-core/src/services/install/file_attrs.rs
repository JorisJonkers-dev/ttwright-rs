// SPDX-License-Identifier: GPL-3.0-only
//! File times (`FileAttrs`): after an install, the manifest sets the modification time of some files in the
//! destination. The game orders archives and plugins by these times, so they are applied, not skipped.

use anyhow::{Context, Result};

use super::post_commands::post_command_path;
use crate::models::FileAttr;
use crate::services::processing::asset::paths::find_file_case_insensitive;

/// Set the modification time of each file the manifest names. Returns `(set, failed)`; a file that is missing
/// counts as failed, with a warning naming it.
///
/// # Errors
/// Never for a single file; failures are counted.
pub fn apply_file_attrs(attrs: &[FileAttr], destination: &str) -> Result<(usize, usize)> {
    let mut set = 0;
    let mut failed = 0;
    for attr in attrs {
        match apply_one(attr, destination) {
            Ok(()) => set += 1,
            Err(e) => {
                tracing::warn!("File time not set: {e:#}");
                failed += 1;
            }
        }
    }
    Ok((set, failed))
}

fn apply_one(attr: &FileAttr, destination: &str) -> Result<()> {
    let raw = attr.value.as_deref().unwrap_or_default();
    let when = attr.last_modified.as_deref().unwrap_or_default();
    let time = chrono::DateTime::parse_from_rfc3339(when)
        .with_context(|| format!("{raw}: the time {when:?} is not an RFC 3339 date"))?;
    let path = raw.replace("%DESTINATION%", destination).replace('\\', "/");
    let path = post_command_path(destination, &path)?;
    let path = if path.exists() {
        path
    } else {
        find_file_case_insensitive(&path)
            .with_context(|| format!("{}: no such file in the destination", path.display()))?
    };
    let file = std::fs::File::options()
        .write(true)
        .open(&path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.set_modified(std::time::SystemTime::from(time))
        .with_context(|| format!("Failed to set the time of {}", path.display()))?;
    tracing::info!("  File time: {} = {}", path.display(), when);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttwright_testkit::{TempDir, write};

    fn attr(value: &str, when: &str) -> FileAttr {
        FileAttr {
            value: Some(value.to_owned()),
            last_modified: Some(when.to_owned()),
            extra: Default::default(),
        }
    }

    #[test]
    fn times_are_set_and_missing_or_outside_files_fail() {
        let dir = TempDir::new("attrs");
        write(&dir.join("Synthetic - Main.bsa"), b"a");
        let dest = dir.path().to_string_lossy().into_owned();
        let attrs = [
            attr(
                "%DESTINATION%\\synthetic - main.BSA",
                "2008-10-01T12:00:00+00:00",
            ),
            attr("%DESTINATION%\\missing.esm", "2008-10-01T12:00:00Z"),
            attr("%DESTINATION%\\..\\outside.txt", "2008-10-01T12:00:00Z"),
            attr("%DESTINATION%\\Synthetic - Main.bsa", "not a date"),
        ];
        let (set, failed) = apply_file_attrs(&attrs, &dest).unwrap();
        assert_eq!((set, failed), (1, 3));
        let modified = std::fs::metadata(dir.join("Synthetic - Main.bsa"))
            .unwrap()
            .modified()
            .unwrap();
        let expected = chrono::DateTime::parse_from_rfc3339("2008-10-01T12:00:00Z").unwrap();
        assert_eq!(modified, std::time::SystemTime::from(expected));
    }
}
