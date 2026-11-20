use super::ManifestLoader;
use crate::models::{PostCommand, TtwManifest};
use crate::services::runtime::Containment;
use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};
use ttwright_contract::REDIRECT_DIR;

/// What the location of a post-installation target is called in the result.
pub(crate) const POST_INSTALL_LOCATION: &str = "(post-installation step)";

/// A file a post-installation step works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PostPath {
    /// The file to work on; always inside the destination.
    pub path: PathBuf,
    /// Set when the manifest named a place outside the destination: that place. `path` is the redirect.
    pub meant: Option<PathBuf>,
}

/// One post-installation command, parsed.
enum Command {
    Delete(PostPath),
    Rename { old: PostPath, new_name: String },
}

impl ManifestLoader {
    /// Get post-installation commands from manifest
    pub fn get_post_commands(manifest: &TtwManifest) -> Vec<PostCommand> {
        manifest.post_commands.clone().unwrap_or_default()
    }

    /// The places the commands name that are outside `destination`, with where each one goes instead
    /// (`_ttwright_game_root/` in the destination). Nothing is touched. A command that does not parse is skipped:
    /// the run reports it when it executes.
    pub fn redirected_post_targets(
        post_commands: &[PostCommand],
        destination: &str,
    ) -> Vec<ttwright_contract::RedirectedFile> {
        post_commands
            .iter()
            .filter_map(|c| c.value.as_deref())
            .filter_map(|value| parse_command(value, destination).ok())
            .map(|command| match command {
                Command::Delete(p) | Command::Rename { old: p, .. } => p,
            })
            .filter_map(|p| redirect_record(&p, destination))
            .collect()
    }

    /// Execute post-installation commands (translated from Windows to Linux)
    /// These are typically rename/delete operations for BSA files.
    pub fn execute_post_commands(
        post_commands: &[PostCommand],
        destination: &str,
    ) -> Result<(usize, usize)> {
        let mut success = 0;
        let mut failed = 0;

        for cmd in post_commands {
            let value = match &cmd.value {
                Some(v) => v,
                None => continue,
            };

            match Self::execute_single_command(value, destination) {
                Ok(_) => success += 1,
                Err(e) => {
                    tracing::warn!("  PostCommand failed: {} - {}", value, e);
                    failed += 1;
                }
            }
        }

        Ok((success, failed))
    }

    /// Execute a single Windows command translated to Linux.
    fn execute_single_command(cmd: &str, destination: &str) -> Result<()> {
        let guard = Containment::new(Path::new(destination));
        match parse_command(cmd, destination)? {
            Command::Delete(target) => {
                let path = target.path;
                // The last line of defence, on the final path (links and `..` resolved).
                guard.check(&path)?;
                if path.exists() {
                    std::fs::remove_file(&path)
                        .with_context(|| format!("Failed to delete: {}", path.display()))?;
                    tracing::info!("  Deleted: {}", path.display());
                }
            }
            Command::Rename { old, new_name } => {
                let old_path = old.path;
                let new_path = old_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join(&new_name);
                guard.check(&old_path)?;
                guard.check(&new_path)?;

                if old_path.exists() {
                    std::fs::rename(&old_path, &new_path).with_context(|| {
                        format!(
                            "Failed to rename: {} -> {}",
                            old_path.display(),
                            new_path.display()
                        )
                    })?;
                    tracing::info!(
                        "  Renamed: {} -> {}",
                        old_path.file_name().unwrap_or_default().to_string_lossy(),
                        new_name
                    );
                } else {
                    tracing::info!("  Skip rename (not found): {}", old_path.display());
                }
            }
        }

        Ok(())
    }
}

fn parse_command(cmd: &str, destination: &str) -> Result<Command> {
    let cmd = cmd.trim();
    let cmd = strip_shell_prefix(cmd);
    let cmd = cmd.replace("%DESTINATION%", destination);
    let cmd = cmd.replace('\\', "/");

    if cmd.get(..4).is_some_and(|v| v.eq_ignore_ascii_case("del ")) {
        let path = cmd[4..].trim().trim_matches('"');
        Ok(Command::Delete(post_command_path(destination, path)?))
    } else if cmd.get(..4).is_some_and(|v| v.eq_ignore_ascii_case("ren ")) {
        let parts: Vec<&str> = cmd[4..]
            .trim()
            .split('"')
            .filter(|s| !s.trim().is_empty())
            .collect();

        if parts.len() >= 2 {
            let old = post_command_path(destination, parts[0].trim())?;
            let new_name = parts[1].trim();
            if !is_plain_filename(new_name) {
                anyhow::bail!("Refusing rename target that is not a plain filename: {new_name}");
            }
            Ok(Command::Rename {
                old,
                new_name: new_name.to_owned(),
            })
        } else {
            anyhow::bail!("ren needs a source and a new name: {cmd}");
        }
    } else {
        // The feature check refuses other commands before the install starts; never skip one silently.
        anyhow::bail!("Unsupported post-install command: {cmd}");
    }
}

/// The result's record of a target the manifest meant outside the destination.
pub(crate) fn redirect_record(
    target: &PostPath,
    destination: &str,
) -> Option<ttwright_contract::RedirectedFile> {
    let meant = target.meant.as_ref()?;
    Some(crate::services::redirected_file(
        ttwright_contract::RedirectedStep::PostInstall,
        POST_INSTALL_LOCATION.to_owned(),
        meant,
        &target.path,
        Path::new(destination),
    ))
}

/// The command after a leading `cmd.exe /C` or `cmd /C`, in any case.
fn strip_shell_prefix(cmd: &str) -> &str {
    let mut rest = cmd;
    for prefix in ["cmd.exe /c ", "cmd /c "] {
        if rest
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
        {
            rest = rest[prefix.len()..].trim_start();
        }
    }
    rest
}

/// The file a post-installation step works on. A path outside `destination` (an absolute path, or `..` out of it)
/// is redirected below `<destination>/_ttwright_game_root/`, keeping its folders; ttwright never works on a file
/// outside the destination.
pub(crate) fn post_command_path(destination: &str, raw_path: &str) -> Result<PostPath> {
    let destination_path = normalize_lexical(PathBuf::from(destination))?;
    let raw = PathBuf::from(raw_path);
    let joined = if raw.is_absolute() {
        raw.clone()
    } else {
        destination_path.join(&raw)
    };
    if let Ok(path) = normalize_lexical(joined.clone())
        && path.starts_with(&destination_path)
    {
        return Ok(PostPath { path, meant: None });
    }
    // Outside: keep the named folders (the plain names; no root, no `..`) under the redirect folder.
    let meant = normalize_lexical(joined.clone()).unwrap_or(joined);
    let kept: PathBuf = meant
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    if kept.as_os_str().is_empty() {
        anyhow::bail!("Refusing post-install path with no file name: {raw_path}");
    }
    Ok(PostPath {
        path: destination_path.join(REDIRECT_DIR).join(kept),
        meant: Some(meant),
    })
}

fn is_plain_filename(name: &str) -> bool {
    let path = PathBuf::from(name);
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn normalize_lexical(path: PathBuf) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                if !normalized.pop() {
                    anyhow::bail!("Refusing path traversal in post-install command");
                }
            }
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::ManifestLoader;
    use crate::models::PostCommand;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ttw_installer_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn post_commands_delete_inside_destination() {
        let dir = test_dir("delete_inside");
        let file = dir.join("delete-me.txt");
        fs::write(&file, b"remove").unwrap();

        let commands = [PostCommand {
            value: Some(format!("cmd.exe /C del \"{}\"", file.display())),
            wait: false,
            hidden: false,
            extra: Default::default(),
        }];

        let (success, failed) =
            ManifestLoader::execute_post_commands(&commands, &dir.to_string_lossy()).unwrap();

        assert_eq!((success, failed), (1, 0));
        assert!(!file.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn post_commands_redirect_delete_outside_destination() {
        let dir = test_dir("delete_outside");
        let outside = std::env::temp_dir().join(format!(
            "ttw_installer_outside_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&outside, b"keep").unwrap();

        let commands = [PostCommand {
            value: Some(format!("cmd.exe /C del \"{}\"", outside.display())),
            wait: false,
            hidden: false,
            extra: Default::default(),
        }];

        let (success, failed) =
            ManifestLoader::execute_post_commands(&commands, &dir.to_string_lossy()).unwrap();

        // The step is redirected into the destination (the file there does not exist: nothing to delete); the file
        // outside is never touched.
        assert_eq!((success, failed), (1, 0));
        assert!(outside.exists());
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn post_commands_reject_rename_to_relative_path() {
        let dir = test_dir("rename_relative_path");
        let file = dir.join("old.txt");
        fs::write(&file, b"keep").unwrap();

        let commands = [PostCommand {
            value: Some(format!(
                "cmd.exe /C ren \"{}\" \"../outside.txt\"",
                file.display()
            )),
            wait: false,
            hidden: false,
            extra: Default::default(),
        }];

        let (success, failed) =
            ManifestLoader::execute_post_commands(&commands, &dir.to_string_lossy()).unwrap();

        assert_eq!((success, failed), (0, 1));
        assert!(file.exists());
        assert!(!dir.join("..").join("outside.txt").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn post_commands_reject_rename_to_current_dir() {
        let dir = test_dir("rename_current_dir");
        let file = dir.join("old.txt");
        fs::write(&file, b"keep").unwrap();

        let commands = [PostCommand {
            value: Some(format!("cmd.exe /C ren \"{}\" \".\"", file.display())),
            wait: false,
            hidden: false,
            extra: Default::default(),
        }];

        let (success, failed) =
            ManifestLoader::execute_post_commands(&commands, &dir.to_string_lossy()).unwrap();

        assert_eq!((success, failed), (0, 1));
        assert!(file.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
