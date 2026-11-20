// SPDX-License-Identifier: GPL-3.0-only
//! The last line of defence of the rule "ttwright never writes outside the destination": a check on the final,
//! resolved path of a file, run right before it is written, deleted or renamed.
//!
//! The check resolves `..` and symbolic links (also a link that points nowhere yet), and a path that does not exist
//! yet is resolved from its deepest existing ancestor. It does not trust how the path was built.

use anyhow::{Result, bail};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The most symbolic links followed while resolving one path; more means a loop.
const MAX_LINK_HOPS: u32 = 40;

enum Part {
    Root(OsString),
    Parent,
    Normal(OsString),
}

fn parts(path: &Path) -> VecDeque<Part> {
    path.components()
        .filter_map(|c| match c {
            Component::Prefix(p) => Some(Part::Root(p.as_os_str().to_owned())),
            Component::RootDir => Some(Part::Root(c.as_os_str().to_owned())),
            Component::CurDir => None,
            Component::ParentDir => Some(Part::Parent),
            Component::Normal(n) => Some(Part::Normal(n.to_owned())),
        })
        .collect()
}

/// The path as the file system resolves it: absolute, without `.` or `..`, every symbolic link followed. The
/// components that do not exist yet are kept as written.
///
/// # Errors
/// A link loop, or a file system error other than "not found".
pub fn resolve_final(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut pending = parts(&absolute);
    let mut out = PathBuf::new();
    let mut hops = 0;
    while let Some(part) = pending.pop_front() {
        match part {
            Part::Root(root) => {
                out.push(root);
            }
            Part::Parent => {
                out.pop();
            }
            Part::Normal(name) => {
                out.push(name);
                match std::fs::symlink_metadata(&out) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        hops += 1;
                        if hops > MAX_LINK_HOPS {
                            return Err(io::Error::other(format!(
                                "too many symbolic links resolving {}",
                                path.display()
                            )));
                        }
                        let target = std::fs::read_link(&out)?;
                        out.pop();
                        if target.is_absolute() {
                            out = PathBuf::new();
                        }
                        for part in parts(&target).into_iter().rev() {
                            pending.push_front(part);
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    // A file where a folder should be, and similar: the write will fail by itself.
                    Err(e) if e.kind() == io::ErrorKind::NotADirectory => {}
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(out)
}

/// A folder that every written, deleted or renamed path must stay inside.
#[derive(Debug, Clone)]
pub struct Containment {
    root: PathBuf,
    resolved: Option<PathBuf>,
}

impl Containment {
    /// Contain paths in `root`, which need not exist yet.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            resolved: resolve_final(root).ok(),
        }
    }

    /// The folder as given.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether `path` resolves to a place inside the folder (the folder itself counts). A path that cannot be
    /// resolved is not inside.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        match (&self.resolved, resolve_final(path)) {
            (Some(root), Ok(path)) => path.starts_with(root),
            _ => false,
        }
    }

    /// Let `path` through when it resolves inside the folder.
    ///
    /// # Errors
    /// When the final path is outside the folder or cannot be resolved; the caller must not touch the path then.
    pub fn check(&self, path: &Path) -> Result<()> {
        if self.contains(path) {
            Ok(())
        } else {
            bail!(
                "Refusing to touch {}: it is outside the destination {}",
                path.display(),
                self.root.display()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ttwright_testkit::TempDir;

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let dir = TempDir::new("containment");
        let dest = dir.join("dest");
        let outside = dir.join("outside");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        (dir, dest, outside)
    }

    #[test]
    fn a_path_inside_passes_also_when_it_does_not_exist_yet() {
        let (_dir, dest, _) = fixture();
        let c = Containment::new(&dest);
        assert!(c.contains(&dest.join("a/b/c.txt")));
        assert!(c.contains(&dest));
        c.check(&dest.join("new.txt")).unwrap();
    }

    #[test]
    fn a_path_outside_is_refused() {
        let (_dir, dest, outside) = fixture();
        let c = Containment::new(&dest);
        let err = c.check(&outside.join("x.txt")).unwrap_err();
        assert!(err.to_string().contains("outside the destination"));
    }

    #[test]
    fn a_sibling_with_the_same_prefix_is_outside() {
        let (dir, dest, _) = fixture();
        let sibling = dir.join("dest-other");
        std::fs::create_dir_all(&sibling).unwrap();
        assert!(!Containment::new(&dest).contains(&sibling.join("x")));
    }

    #[test]
    fn dot_dot_escapes_are_refused_and_dot_dot_inside_is_allowed() {
        let (_dir, dest, _) = fixture();
        let c = Containment::new(&dest);
        assert!(!c.contains(&dest.join("../outside/x.txt")));
        assert!(!c.contains(&dest.join("a/../../outside/x.txt")));
        assert!(c.contains(&dest.join("a/../b.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_inside_pointing_outside_is_refused() {
        let (_dir, dest, outside) = fixture();
        std::os::unix::fs::symlink(&outside, dest.join("link")).unwrap();
        let c = Containment::new(&dest);
        assert!(!c.contains(&dest.join("link/x.txt")));
        assert!(!c.contains(&dest.join("link/deeper/not/yet/there.txt")));
        // `..` after a link is resolved from where the link leads, not from the link's folder.
        assert!(!c.contains(&dest.join("link/../x.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_symlink_and_a_dangling_symlink_pointing_outside_are_refused() {
        let (_dir, dest, outside) = fixture();
        std::fs::write(outside.join("real.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(outside.join("real.txt"), dest.join("file-link")).unwrap();
        std::os::unix::fs::symlink(outside.join("missing.txt"), dest.join("dangling")).unwrap();
        let c = Containment::new(&dest);
        assert!(!c.contains(&dest.join("file-link")));
        assert!(!c.contains(&dest.join("dangling")));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_stays_inside_passes_and_a_loop_is_refused() {
        let (_dir, dest, _) = fixture();
        std::fs::create_dir_all(dest.join("real")).unwrap();
        std::os::unix::fs::symlink(dest.join("real"), dest.join("alias")).unwrap();
        std::os::unix::fs::symlink(dest.join("b"), dest.join("a")).unwrap();
        std::os::unix::fs::symlink(dest.join("a"), dest.join("b")).unwrap();
        let c = Containment::new(&dest);
        assert!(c.contains(&dest.join("alias/x.txt")));
        assert!(!c.contains(&dest.join("a/x.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn a_destination_that_is_itself_a_link_contains_what_it_leads_to() {
        let (dir, dest, _) = fixture();
        let link = dir.join("dest-link");
        std::os::unix::fs::symlink(&dest, &link).unwrap();
        let c = Containment::new(&link);
        assert!(c.contains(&dest.join("x.txt")));
        assert!(c.contains(&link.join("x.txt")));
    }
}
