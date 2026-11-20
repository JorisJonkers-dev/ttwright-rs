// SPDX-License-Identifier: GPL-3.0-only
//! What an install supports: the TTW versions and the manifest features. The Toolbox shows it from
//! `ttwright contracts --json`; the installer refuses anything else with `unsupported_feature` or
//! `unsupported_version` before it writes a file.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A package family this build installs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SupportedPackage {
    /// The package's name.
    pub name: String,
    /// The `major.minor` versions whose manifests this build reads and installs, oldest first. Empty: any version
    /// whose manifest uses only supported features.
    pub versions: Vec<String>,
    /// The newest public release this build was written against, when one is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_release: Option<String>,
}

/// One manifest feature this build implements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Feature {
    /// A stable id, for example `asset_op:2`; an `unsupported_feature` error names ids of the same form.
    pub id: String,
    /// What it is, for people.
    pub description: String,
}

/// The packages and manifest features an install supports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Supports {
    /// Package families. A TTW package outside the listed versions is refused with `unsupported_version` unless
    /// `--allow-unsupported-version` is given.
    pub packages: Vec<SupportedPackage>,
    /// Manifest features. A manifest that uses any other is refused with `unsupported_feature`.
    pub features: Vec<Feature>,
}

/// The TTW versions (`major.minor`) whose manifests this build installs.
pub const TTW_VERSIONS: &[&str] = &["3.3", "3.4"];
/// The current public TTW release this build was written against.
pub const TTW_CURRENT_RELEASE: &str = "3.4.0";

/// The manifest features this build implements, by id.
pub const FEATURES: &[(&str, &str)] = &[
    (
        "manifest_field:Package",
        "package title, version and author",
    ),
    ("manifest_field:Variables", "variables per profile"),
    ("manifest_field:Locations", "locations per profile"),
    ("manifest_field:Tags", "asset tags (informational)"),
    ("manifest_field:Assets", "asset operations"),
    ("manifest_field:Checks", "pre-installation checks"),
    (
        "manifest_field:FileAttrs",
        "modification times set after the install",
    ),
    ("manifest_field:PostCommands", "post-installation commands"),
    ("asset_op:0", "copy a file from a game folder or archive"),
    ("asset_op:1", "a new file from the package"),
    ("asset_op:2", "a VCDIFF (xdelta3) patch of a game file"),
    ("asset_op:4", "resample Ogg Vorbis audio (OggEnc2)"),
    (
        "asset_op:5",
        "decode and encode audio to Ogg Vorbis, WAV or MP3",
    ),
    ("audio_param:-f", "target sample rate in Hz"),
    ("audio_param:-q", "Ogg Vorbis quality, 0 to 10"),
    (
        "audio_param:-c",
        "output channel count, 1 or 2 (mono and stereo convert into each other)",
    ),
    (
        "audio_param:-fmt",
        "output format (ogg, wav or mp3), checked against the target file's extension",
    ),
    (
        "audio_param:-b",
        "MP3 bit rate in kbit/s (8 to 320, the LAME rates)",
    ),
    ("audio_format:ogg", "Ogg Vorbis output"),
    ("audio_format:wav", "WAV output"),
    ("audio_format:mp3", "MP3 output (LAME)"),
    ("location_type:0", "a folder"),
    ("location_type:1", "an archive (BSA) read from"),
    ("location_type:2", "an archive (BSA) written to"),
    ("variable_type:0", "a string, may name other variables"),
    (
        "variable_type:4",
        "a registry path, replaced by the game copy given on the command line",
    ),
    ("variable:%DESTINATION%", "the destination folder"),
    (
        "variable:%FO3ROOT%",
        "the Fallout 3 copy (also %FO3DATA%, %FALLOUT3ROOT%, %FALLOUT3DATA%)",
    ),
    (
        "variable:%FNVROOT%",
        "the Fallout: New Vegas copy (also %FNVDATA%, %FALLOUTNVROOT%, %FALLOUTNVDATA%)",
    ),
    (
        "variable:%TES4ROOT%",
        "the Oblivion copy (also %TES4DATA%, %OBLIVIONROOT%, %OBLIVIONDATA%)",
    ),
    (
        "check_type:0",
        "a file exists, or matches an MD5, SHA-1 or CRC32 checksum",
    ),
    ("check_type:1", "free space at a location"),
    ("check_type:2", "the destination is not under Program Files"),
    ("post_command:del", "delete a file in the destination"),
    ("post_command:ren", "rename a file in the destination"),
];

/// Whether this build implements the feature `id`.
#[must_use]
pub fn is_supported(id: &str) -> bool {
    FEATURES.iter().any(|(known, _)| *known == id)
}

/// What this build supports.
#[must_use]
pub fn supports() -> Supports {
    Supports {
        packages: vec![
            SupportedPackage {
                name: "Tale of Two Wastelands".to_owned(),
                versions: TTW_VERSIONS.iter().map(|v| (*v).to_owned()).collect(),
                current_release: Some(TTW_CURRENT_RELEASE.to_owned()),
            },
            SupportedPackage {
                name: "other MPI packages".to_owned(),
                versions: Vec::new(),
                current_release: None,
            },
        ],
        features: FEATURES
            .iter()
            .map(|(id, description)| Feature {
                id: (*id).to_owned(),
                description: (*description).to_owned(),
            })
            .collect(),
    }
}

/// How a package's version relates to the supported list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSupport {
    /// Not a TTW package: no version list applies.
    NotTtw,
    /// A TTW version this build supports.
    Supported,
    /// A TTW version this build does not support, as `major.minor` or the raw text when it has no number.
    Unsupported(String),
}

/// Whether a package with this manifest title and version is a supported TTW release.
#[must_use]
pub fn ttw_version_support(title: &str, version: &str) -> VersionSupport {
    let lower = title.to_ascii_lowercase();
    let is_ttw =
        lower.contains("tale of two wastelands") || lower.split_whitespace().any(|w| w == "ttw");
    if !is_ttw {
        return VersionSupport::NotTtw;
    }
    let numbers: Vec<&str> = version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', ' ', '-'])
        .take(2)
        .collect();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if numbers.len() == 2 && numbers.iter().all(|n| digits(n)) {
        let major_minor = format!("{}.{}", numbers[0], numbers[1]);
        if TTW_VERSIONS.contains(&major_minor.as_str()) {
            return VersionSupport::Supported;
        }
        return VersionSupport::Unsupported(major_minor);
    }
    VersionSupport::Unsupported(version.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttw_versions() {
        let t = "Tale of Two Wastelands";
        assert_eq!(ttw_version_support(t, "3.4.0"), VersionSupport::Supported);
        assert_eq!(ttw_version_support(t, "3.4"), VersionSupport::Supported);
        assert_eq!(
            ttw_version_support(t, "v3.3.3.1"),
            VersionSupport::Supported
        );
        assert_eq!(
            ttw_version_support(t, "3.5.0"),
            VersionSupport::Unsupported("3.5".into())
        );
        assert_eq!(
            ttw_version_support("TTW", "beta"),
            VersionSupport::Unsupported("beta".into())
        );
        assert_eq!(
            ttw_version_support("Synthetic Archive Decompressor", "1.0"),
            VersionSupport::NotTtw
        );
    }

    #[test]
    fn features_are_unique_and_listed() {
        let mut ids: Vec<_> = FEATURES.iter().map(|(id, _)| *id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
        assert!(is_supported("asset_op:2"));
        assert!(!is_supported("asset_op:3"));
        let s = supports();
        assert_eq!(s.features.len(), n);
        assert_eq!(s.packages[0].current_release.as_deref(), Some("3.4.0"));
        assert!(s.packages[1].versions.is_empty());
    }
}
