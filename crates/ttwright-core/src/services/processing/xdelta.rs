use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Applies VCDIFF (xdelta3) patches with oxidelta, in memory: no subprocess, no temporary files, no
/// `xdelta3` program to find. ttwright starts no other program (the Component rule of the Toolbox).
pub struct XdeltaManager {
    /// Directory for temp files (uses output dir, not system temp)
    _staging_dir: PathBuf,
}

impl XdeltaManager {
    /// The patcher, for installs into `staging_dir`. Always available: oxidelta is built in.
    pub fn ensure_available(staging_dir: PathBuf) -> Result<Self> {
        tracing::info!("Patching with oxidelta (pure Rust VCDIFF)");
        Ok(Self {
            _staging_dir: staging_dir,
        })
    }

    /// What applies the patches.
    pub fn verify(&self) -> Result<String> {
        Ok("oxidelta (pure Rust VCDIFF)".to_string())
    }

    /// Apply a delta patch to create output file
    pub fn apply_patch(&self, source: &Path, patch: &Path, output: &Path) -> Result<()> {
        let source_data = fs::read(source)
            .with_context(|| format!("Failed to read source: {}", source.display()))?;
        let patch_data = fs::read(patch)
            .with_context(|| format!("Failed to read patch: {}", patch.display()))?;

        let result = self.apply_patch_from_bytes(&source_data, &patch_data)?;

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output, &result)?;
        Ok(())
    }

    /// Apply a delta patch from bytes (pure Rust, no subprocess, no temp files).
    /// Uses oxidelta for VCDIFF decoding directly in memory.
    pub fn apply_patch_from_bytes(&self, source_data: &[u8], patch_data: &[u8]) -> Result<Vec<u8>> {
        oxidelta::compress::decoder::decode_all(source_data, patch_data)
            .context("VCDIFF patch failed (oxidelta)")
    }
}

#[cfg(test)]
mod tests {
    use super::XdeltaManager;
    use ttwright_testkit::{TempDir, covers, ids, vcdiff, write};

    #[test]
    fn a_patch_applies_in_memory_and_on_disk() {
        covers(&[ids::install::features::ASSET_OP_2]);
        let dir = TempDir::new("xdelta");
        let x = XdeltaManager::ensure_available(dir.path().to_path_buf()).unwrap();
        assert!(x.verify().unwrap().contains("oxidelta"));
        let patch = vcdiff(b"synthetic source", b"synthetic target");
        assert_eq!(
            x.apply_patch_from_bytes(b"synthetic source", &patch)
                .unwrap(),
            b"synthetic target"
        );
        write(&dir.join("src.bin"), b"synthetic source");
        write(&dir.join("p.xd3"), &patch);
        x.apply_patch(
            &dir.join("src.bin"),
            &dir.join("p.xd3"),
            &dir.join("out/t.bin"),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(dir.join("out/t.bin")).unwrap(),
            b"synthetic target"
        );
        assert!(x.apply_patch_from_bytes(b"x", b"not a patch").is_err());
        assert!(
            x.apply_patch(&dir.join("none"), &dir.join("p.xd3"), &dir.join("o"))
                .is_err()
        );
    }
}
