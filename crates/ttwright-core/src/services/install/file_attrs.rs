// SPDX-License-Identifier: GPL-3.0-only
//! File times (`FileAttrs`): after an install, the manifest sets the modification time of some files in the
//! destination. The game orders archives and plugins by these times, so they are applied, not skipped.

use anyhow::{Context, Result};

use super::post_commands::{post_command_path, redirect_record};
use crate::models::FileAttr;
use crate::services::processing::asset::paths::find_file_case_insensitive;
use crate::services::runtime::Containment;
use ttwright_contract::RedirectedFile;

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

/// The files the manifest sets a time on that are outside `destination`, with where each one goes instead
/// (`_ttwright_game_root/` in the destination). Nothing is touched.
#[must_use]
pub fn redirected_attr_targets(attrs: &[FileAttr], destination: &str) -> Vec<RedirectedFile> {
    attrs
        .iter()
        .filter_map(|attr| {
            let raw = attr.value.as_deref().unwrap_or_default();
            let path = raw.replace("%DESTINATION%", destination).replace('\\', "/");
            redirect_record(&post_command_path(destination, &path).ok()?, destination)
        })
        .collect()
}

/// The instant as a `SystemTime`, before 1970 too (`time` has no conversion of its own).
fn system_time(instant: time::OffsetDateTime) -> std::time::SystemTime {
    let secs = instant.unix_timestamp();
    let nanos = instant.nanosecond();
    if secs >= 0 {
        std::time::UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos)
    } else {
        std::time::UNIX_EPOCH - std::time::Duration::from_secs(secs.unsigned_abs())
            + std::time::Duration::from_nanos(u64::from(nanos))
    }
}

fn apply_one(attr: &FileAttr, destination: &str) -> Result<()> {
    let raw = attr.value.as_deref().unwrap_or_default();
    let when = attr.last_modified.as_deref().unwrap_or_default();
    let instant = time::OffsetDateTime::parse(when, &time::format_description::well_known::Rfc3339)
        .with_context(|| format!("{raw}: the time {when:?} is not an RFC 3339 date"))?;
    let path = raw.replace("%DESTINATION%", destination).replace('\\', "/");
    let path = post_command_path(destination, &path)?.path;
    let guard = Containment::new(std::path::Path::new(destination));
    guard.check(&path)?;
    let path = if path.exists() {
        path
    } else {
        find_file_case_insensitive(&path)
            .with_context(|| format!("{}: no such file in the destination", path.display()))?
    };
    // The file found case-insensitively, and links, are checked again on the final path.
    guard.check(&path)?;
    let file = std::fs::File::options()
        .write(true)
        .open(&path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.set_modified(system_time(instant))
        .with_context(|| format!("Failed to set the time of {}", path.display()))?;
    tracing::info!("  File time: {} = {}", path.display(), when);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttwright_testkit::{TempDir, covers, ids, write};

    fn attr(value: &str, when: &str) -> FileAttr {
        FileAttr {
            value: Some(value.to_owned()),
            last_modified: Some(when.to_owned()),
            extra: Default::default(),
        }
    }

    #[test]
    fn system_time_is_exact_before_and_after_the_epoch() {
        let before = time::OffsetDateTime::parse(
            "1969-12-31T23:59:59.5Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        assert_eq!(
            system_time(before),
            std::time::UNIX_EPOCH - std::time::Duration::from_millis(500)
        );
        let after = time::OffsetDateTime::parse(
            "1970-01-01T00:00:01.25Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        assert_eq!(
            system_time(after),
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(1250)
        );
    }

    #[test]
    fn times_are_set_and_missing_or_outside_files_fail() {
        covers(&[
            ids::install::features::MANIFEST_FIELD_FILEATTRS,
            ids::install::features::VARIABLE__DESTINATION_,
        ]);
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
        let expected = time::OffsetDateTime::parse(
            "2008-10-01T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        assert_eq!(modified, system_time(expected));
    }
}
