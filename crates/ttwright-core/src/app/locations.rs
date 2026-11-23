// SPDX-License-Identifier: GPL-3.0-only
//! The `--location NAME=PATH` bindings for source locations a package declares in its manifest.

use std::path::{Path, PathBuf};

use ttwright_contract::ErrorCode;

use crate::app::InstallError;
use crate::services::Containment;

/// One binding of a location's declared name to a folder (or an archive) on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationBinding {
    /// The name as the manifest declares it.
    pub name: String,
    /// The folder to read from. ttwright never writes into it.
    pub path: PathBuf,
}

/// The bindings given to one install, parsed from repeated `NAME=PATH` values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocationBindings {
    entries: Vec<LocationBinding>,
}

impl LocationBindings {
    /// Parse repeated `NAME=PATH` values. A malformed value, a duplicate name or a missing path is a usage error.
    ///
    /// # Errors
    /// `usage` naming the value that could not be parsed.
    pub fn parse(values: &[String]) -> Result<Self, InstallError> {
        let mut entries: Vec<LocationBinding> = Vec::new();
        for value in values {
            let Some((name, path)) = value.split_once('=') else {
                return Err(usage(format!(
                    "invalid --location {value:?}: expected NAME=PATH"
                )));
            };
            let (name, path) = (name.trim(), path.trim());
            if name.is_empty() || path.is_empty() {
                return Err(usage(format!(
                    "invalid --location {value:?}: both NAME and PATH are needed"
                )));
            }
            let path = PathBuf::from(path);
            if !path.exists() {
                return Err(usage(format!(
                    "the source folder for location {name} does not exist: {}",
                    path.display()
                )));
            }
            if entries
                .iter()
                .any(|binding| binding.name.eq_ignore_ascii_case(name))
            {
                return Err(usage(format!("location {name} was bound more than once")));
            }
            entries.push(LocationBinding {
                name: name.to_owned(),
                path,
            });
        }
        Ok(Self { entries })
    }

    /// The folder bound to `name`, when there is one. Names are matched without regard to case.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Path> {
        if name.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .find(|binding| binding.name.eq_ignore_ascii_case(name))
            .map(|binding| binding.path.as_path())
    }

    /// Whether no bindings were given.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every binding, in the order it was given.
    pub fn iter(&self) -> impl Iterator<Item = &LocationBinding> {
        self.entries.iter()
    }
}

/// Refuse a binding whose folder is inside `destination` or contains it, before any file is written. Both paths
/// are resolved (`..` and symbolic links) before they are compared, so an alias cannot make a bound folder a
/// write target: a bound folder is only ever read from.
///
/// # Errors
/// `usage` naming the location and the overlapping paths.
pub fn check_bindings_outside_destination(
    bindings: &LocationBindings,
    destination: &Path,
) -> Result<(), InstallError> {
    let inside_destination = Containment::new(destination);
    for binding in bindings.iter() {
        if inside_destination.contains(&binding.path)
            || Containment::new(&binding.path).contains(destination)
        {
            return Err(usage(format!(
                "--location {} is bound to {}, which is inside or contains the destination {}; bound folders are read only",
                binding.name,
                binding.path.display(),
                destination.display()
            )));
        }
    }
    Ok(())
}

fn usage(message: impl Into<String>) -> InstallError {
    InstallError::new(ErrorCode::Usage, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn bindings_are_names_and_existing_paths() {
        let dir = ttwright_testkit::TempDir::new("bindings");
        let path = dir.path().to_path_buf();
        ttwright_testkit::write(&path.join("a.dds"), b"x");
        let given = path.to_string_lossy().to_string();
        let bindings =
            LocationBindings::parse(&values(&[&format!("My Textures={given}"), "Other=."]))
                .unwrap();
        assert_eq!(bindings.get("my textures"), Some(path.as_path()));
        assert_eq!(bindings.get("MY TEXTURES"), Some(path.as_path()));
        assert_eq!(bindings.get("missing"), None);
        assert_eq!(bindings.get(""), None);
        assert!(!bindings.is_empty());
        assert_eq!(bindings.iter().count(), 2);
    }

    #[test]
    fn a_malformed_binding_is_a_usage_error() {
        assert_eq!(
            LocationBindings::parse(&values(&["no-equals"]))
                .unwrap_err()
                .code,
            ErrorCode::Usage
        );
        assert_eq!(
            LocationBindings::parse(&values(&["=path"]))
                .unwrap_err()
                .code,
            ErrorCode::Usage
        );
        assert_eq!(
            LocationBindings::parse(&values(&["NAME="]))
                .unwrap_err()
                .code,
            ErrorCode::Usage
        );
        assert_eq!(
            LocationBindings::parse(&values(&["NAME=no/such/folder"]))
                .unwrap_err()
                .code,
            ErrorCode::Usage
        );
        assert_eq!(
            LocationBindings::parse(&values(&["NAME=.", "name=."]))
                .unwrap_err()
                .code,
            ErrorCode::Usage
        );
    }

    fn overlapping(dir: &ttwright_testkit::TempDir) -> (PathBuf, PathBuf) {
        let dest = dir.join("dest");
        let source = dir.join("source");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        (dest, source)
    }

    #[test]
    fn a_bound_folder_inside_or_above_the_destination_is_refused() {
        let dir = ttwright_testkit::TempDir::new("bindings-overlap");
        let (dest, source) = overlapping(&dir);
        std::fs::create_dir_all(dest.join("downloads")).unwrap();

        let bind = |path: &Path| {
            LocationBindings::parse(&values(&[&format!("Textures={}", path.display())])).unwrap()
        };
        let err =
            check_bindings_outside_destination(&bind(&dest.join("downloads")), &dest).unwrap_err();
        assert_eq!(err.code, ErrorCode::Usage);
        assert!(err.message.contains("Textures"), "{}", err.message);
        assert!(
            check_bindings_outside_destination(&bind(dir.path()), &dest).is_err(),
            "a bound folder containing the destination is refused"
        );
        check_bindings_outside_destination(&bind(&source), &dest).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_alias_or_dot_dot_cannot_hide_an_overlap_with_the_destination() {
        let dir = ttwright_testkit::TempDir::new("bindings-canonical");
        let (dest, source) = overlapping(&dir);
        let alias = source.join("alias");
        std::os::unix::fs::symlink(&dest, &alias).unwrap();
        let bindings =
            LocationBindings::parse(&values(&[&format!("Textures={}", alias.display())])).unwrap();
        assert!(check_bindings_outside_destination(&bindings, &dest).is_err());

        std::fs::create_dir_all(dest.join("inside")).unwrap();
        std::fs::create_dir_all(dest.join("textures")).unwrap();
        let bindings = LocationBindings::parse(&values(&[&format!(
            "Textures={}",
            dest.join("inside/../textures").display()
        )]))
        .unwrap();
        assert!(check_bindings_outside_destination(&bindings, &dest).is_err());
    }
}
