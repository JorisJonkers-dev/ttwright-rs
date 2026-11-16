// SPDX-License-Identifier: GPL-3.0-only
//! The feature check that runs before an install writes anything: every field, type, operation, parameter,
//! variable and command the manifest uses must be one this build implements (`ttwright_contract::FEATURES`).
//! Anything else stops the install with `unsupported_feature` naming it, instead of being skipped.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Map, Value};
use ttwright_contract::{ErrorCode, is_supported};

use crate::app::InstallError;
use crate::models::{Asset, TtwManifest};
use crate::services::AudioFormat;

/// The game copies a manifest needs, from the variables its locations use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameNeeds {
    /// `%FO3ROOT%` and its aliases.
    pub fallout3: bool,
    /// `%FNVROOT%` and its aliases.
    pub falloutnv: bool,
    /// `%TES4ROOT%` and its aliases.
    pub oblivion: bool,
}

/// The variables the installer fills in itself, and the game each belongs to (`None` for the destination).
const BUILTIN: &[(&str, Option<Game>)] = &[
    ("DESTINATION", None),
    ("FO3ROOT", Some(Game::Fallout3)),
    ("FO3DATA", Some(Game::Fallout3)),
    ("FALLOUT3ROOT", Some(Game::Fallout3)),
    ("FALLOUT3DATA", Some(Game::Fallout3)),
    ("FNVROOT", Some(Game::FalloutNv)),
    ("FNVDATA", Some(Game::FalloutNv)),
    ("FALLOUTNVROOT", Some(Game::FalloutNv)),
    ("FALLOUTNVDATA", Some(Game::FalloutNv)),
    ("TES4ROOT", Some(Game::Oblivion)),
    ("TES4DATA", Some(Game::Oblivion)),
    ("OBLIVIONROOT", Some(Game::Oblivion)),
    ("OBLIVIONDATA", Some(Game::Oblivion)),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Game {
    Fallout3,
    FalloutNv,
    Oblivion,
}

fn unsupported(feature: String, what: &str) -> InstallError {
    InstallError::unsupported(
        feature.clone(),
        format!("The manifest uses {what} `{feature}`, which this ttwright does not implement"),
    )
}

fn invalid(message: String) -> InstallError {
    InstallError::new(ErrorCode::ManifestInvalid, message)
}

fn no_extra(extra: &Map<String, Value>, kind: &str) -> Result<(), InstallError> {
    match extra.keys().next() {
        Some(key) => Err(unsupported(format!("{kind}_field:{key}"), "the field")),
        None => Ok(()),
    }
}

fn known(prefix: &str, value: impl std::fmt::Display, what: &str) -> Result<(), InstallError> {
    let id = format!("{prefix}:{value}");
    if is_supported(&id) {
        Ok(())
    } else {
        Err(unsupported(id, what))
    }
}

/// The `%NAME%` tokens in `text`.
#[must_use]
pub fn variable_tokens(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split('%').collect();
    let mut tokens = Vec::new();
    let mut i = 1;
    while i + 1 < parts.len() {
        let name = parts[i];
        if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            tokens.push(name.to_ascii_uppercase());
            i += 2;
        } else {
            i += 1;
        }
    }
    tokens
}

/// The verb of a post-installation command (`del`, `ren`, ...), lower case, after the `cmd.exe /C` prefix.
#[must_use]
pub fn post_command_verb(command: &str) -> String {
    let mut rest = command.trim();
    for prefix in ["cmd.exe /c ", "cmd /c "] {
        if rest.len() >= prefix.len() && rest[..prefix.len()].eq_ignore_ascii_case(prefix) {
            rest = rest[prefix.len()..].trim_start();
        }
    }
    rest.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Check every feature the manifest uses with `profile` and the parsed `assets`, and say which game copies it
/// needs.
///
/// # Errors
/// `unsupported_feature` naming the first unsupported feature, or `manifest_invalid` for an asset that points at
/// a location that does not exist or a file time that cannot be read.
pub fn check_manifest(
    manifest: &TtwManifest,
    assets: &[Asset],
    profile: usize,
) -> Result<GameNeeds, InstallError> {
    no_extra(&manifest.extra, "manifest")?;
    for key in [
        "Package",
        "Variables",
        "Locations",
        "Tags",
        "Assets",
        "Checks",
        "FileAttrs",
        "PostCommands",
    ] {
        debug_assert!(is_supported(&format!("manifest_field:{key}")));
    }

    let locations = manifest
        .locations
        .as_ref()
        .and_then(|l| l.get(profile))
        .cloned()
        .unwrap_or_default();
    let variables = manifest
        .variables
        .as_ref()
        .and_then(|v| v.get(profile).or_else(|| v.first()))
        .cloned()
        .unwrap_or_default();

    for location in &locations {
        no_extra(&location.extra, "location")?;
        known("location_type", location.loc_type, "the location type")?;
    }
    for variable in &variables {
        no_extra(&variable.extra, "variable")?;
        // Personal-folder and local-application-data variables (types 1 and 2) are allowed while no location
        // uses them; a location that does is refused below, by the variable it names.
        if !matches!(variable.var_type, 1 | 2) {
            known("variable_type", variable.var_type, "the variable type")?;
        }
    }
    for check in manifest.checks.iter().flatten() {
        no_extra(&check.extra, "check")?;
        known("check_type", check.check_type, "the check type")?;
        if usize::try_from(check.loc).map_or(true, |i| i >= locations.len()) {
            return Err(invalid(format!(
                "a check names location {}, which the manifest does not define",
                check.loc
            )));
        }
    }
    for command in manifest.post_commands.iter().flatten() {
        no_extra(&command.extra, "post_command")?;
        let text = command.value.as_deref().unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        known(
            "post_command",
            post_command_verb(text),
            "the post-installation command",
        )?;
        for token in variable_tokens(text) {
            if token != "DESTINATION" {
                return Err(unsupported(format!("variable:%{token}%"), "the variable"));
            }
        }
    }
    for attr in manifest.file_attrs.iter().flatten() {
        no_extra(&attr.extra, "file_attr")?;
        let when = attr.last_modified.as_deref().unwrap_or_default();
        if chrono::DateTime::parse_from_rfc3339(when).is_err() {
            return Err(invalid(format!(
                "the file time {when:?} is not an RFC 3339 date"
            )));
        }
        for token in variable_tokens(attr.value.as_deref().unwrap_or_default()) {
            if token != "DESTINATION" {
                return Err(unsupported(format!("variable:%{token}%"), "the variable"));
            }
        }
    }

    for asset in assets {
        known("asset_op", asset.op_type, "the asset operation")?;
        for loc in [asset.source_loc, asset.target_loc] {
            if usize::try_from(loc).map_or(true, |i| i >= locations.len()) {
                return Err(invalid(format!(
                    "asset {} names location {loc}, which the manifest does not define",
                    asset.index
                )));
            }
        }
        if matches!(asset.op_type, 4 | 5) {
            for token in asset.params.split_whitespace() {
                let key = token.split(':').next().unwrap_or(token);
                known("audio_param", key, "the audio parameter")?;
            }
        }
        if asset.op_type == 5 {
            let target = asset.effective_target_path();
            let ext = Path::new(&target.replace('\\', "/"))
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if AudioFormat::from_extension(&ext).is_none() {
                return Err(unsupported(
                    format!("audio_format:{ext}"),
                    "the audio format",
                ));
            }
        }
    }

    // Variables: every token a location uses must resolve to a game copy, the destination or a string variable.
    let mut needs = GameNeeds::default();
    let mut seen = BTreeSet::new();
    let mut pending: Vec<String> = locations
        .iter()
        .flat_map(|l| variable_tokens(l.value.as_deref().unwrap_or_default()))
        .collect();
    while let Some(token) = pending.pop() {
        if !seen.insert(token.clone()) {
            continue;
        }
        if let Some((_, game)) = BUILTIN.iter().find(|(name, _)| *name == token) {
            match game {
                Some(Game::Fallout3) => needs.fallout3 = true,
                Some(Game::FalloutNv) => needs.falloutnv = true,
                Some(Game::Oblivion) => needs.oblivion = true,
                None => {}
            }
            continue;
        }
        let defined = variables.iter().find(|v| {
            v.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(&token))
        });
        match defined {
            Some(v) if v.var_type == 0 && v.value.is_some() => {
                pending.extend(variable_tokens(v.value.as_deref().unwrap_or_default()));
            }
            Some(v) if v.var_type != 0 && v.var_type != 4 => {
                return Err(unsupported(
                    format!("variable_type:{}", v.var_type),
                    "the variable type",
                ));
            }
            _ => return Err(unsupported(format!("variable:%{token}%"), "the variable")),
        }
    }
    Ok(needs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ManifestLoader;
    use serde_json::json;
    use ttwright_testkit::Manifest;

    fn run(m: &Manifest) -> Result<GameNeeds, InstallError> {
        let manifest =
            ManifestLoader::load_from_bytes(&serde_json::to_vec(&m.build()).unwrap()).unwrap();
        let assets = ManifestLoader::parse_assets(&manifest).unwrap();
        check_manifest(&manifest, &assets, 0)
    }

    fn feature(m: &Manifest) -> String {
        let e = run(m).unwrap_err();
        assert_eq!(e.code, ErrorCode::UnsupportedFeature, "{e}");
        e.feature.unwrap()
    }

    fn base() -> Manifest {
        let mut m = Manifest::new("Tale of Two Wastelands", "3.4.0");
        m.variable("FO3ROOT", 4, Some("HKLM\\Software\\x"));
        m.variable("SUB", 0, Some("%DESTINATION%\\Sub"));
        m.location("FO3 Data", 0, "%FO3DATA%");
        m.location("Out", 0, "%SUB%");
        m.archive_target("Out.bsa", "%DESTINATION%\\Out.bsa");
        m.asset(0, "", 0, 1, "a.txt", None);
        m.asset(
            5,
            "-f:24000 -q:5",
            0,
            1,
            "sound\\a.wav",
            Some("sound\\a.ogg"),
        );
        m.post_command("cmd.exe /C ren \"%DESTINATION%\\a\" \"b\"");
        m.file_attr("%DESTINATION%\\b", "2010-10-19T12:00:00+02:00");
        m.check(json!({"Type": 0, "Loc": 0, "File": "a.txt", "Checksums": "00000000"}));
        m
    }

    #[test]
    fn a_manifest_of_supported_features_passes_and_names_its_games() {
        let needs = run(&base()).unwrap();
        assert_eq!(
            needs,
            GameNeeds {
                fallout3: true,
                falloutnv: false,
                oblivion: false
            }
        );
    }

    #[test]
    fn unknown_fields_are_named() {
        let mut m = base();
        m.set("Signature", json!("x"));
        assert_eq!(feature(&m), "manifest_field:Signature");
        let mut m = base();
        let mut v = m.build();
        v["Locations"][0][0]["Mirror"] = json!(true);
        m.set("Locations", v["Locations"].clone());
        assert_eq!(feature(&m), "location_field:Mirror");
    }

    #[test]
    fn unknown_types_operations_and_commands_are_named() {
        let mut m = base();
        m.asset(3, "", 0, 1, "voice.xwm", Some("voice.fuz"));
        assert_eq!(feature(&m), "asset_op:3");
        let mut m = base();
        m.location("Odd", 7, "x");
        assert_eq!(feature(&m), "location_type:7");
        let mut m = base();
        m.variable("Q", 3, Some("x"));
        assert_eq!(feature(&m), "variable_type:3");
        let mut m = base();
        m.check(json!({"Type": 9, "Loc": 0}));
        assert_eq!(feature(&m), "check_type:9");
        let mut m = base();
        m.post_command("cmd.exe /C copy \"%DESTINATION%\\a\" \"%DESTINATION%\\b\"");
        assert_eq!(feature(&m), "post_command:copy");
        let mut m = base();
        m.post_command("cmd.exe /C del \"%FO3ROOT%\\a\"");
        assert_eq!(feature(&m), "variable:%FO3ROOT%");
        let mut m = base();
        m.file_attr("%FNVROOT%\\x", "2010-10-19T12:00:00Z");
        assert_eq!(feature(&m), "variable:%FNVROOT%");
    }

    #[test]
    fn audio_parameters_and_formats_are_checked() {
        let mut m = base();
        m.asset(4, "-f:22050 -b:96", 0, 1, "a.ogg", None);
        assert_eq!(feature(&m), "audio_param:-b");
        let mut m = base();
        m.asset(5, "", 0, 1, "a.wav", Some("a.xwm"));
        assert_eq!(feature(&m), "audio_format:xwm");
    }

    #[test]
    fn variables_must_resolve() {
        let mut m = base();
        m.location("Odd", 0, "%NOWHERE%\\x");
        assert_eq!(feature(&m), "variable:%NOWHERE%");
        let mut m = base();
        m.variable("DOCS", 1, None);
        assert!(
            run(&m).is_ok(),
            "an unused personal-folder variable is fine"
        );
        m.location("Docs", 0, "%DOCS%\\My Games");
        assert_eq!(feature(&m), "variable_type:1");
        let mut m = base();
        m.variable("GAMEKEY", 4, Some("HKLM\\x"));
        m.location("Reg", 0, "%GAMEKEY%");
        assert_eq!(feature(&m), "variable:%GAMEKEY%");
        let mut m = base();
        m.location("NV", 1, "%FNVDATA%\\Synthetic.bsa");
        m.location("Ob", 0, "%oblivionroot%");
        let needs = run(&m).unwrap();
        assert!(needs.fallout3 && needs.falloutnv && needs.oblivion);
    }

    #[test]
    fn bad_references_and_dates_are_invalid() {
        let mut m = base();
        m.asset(0, "", 0, 9, "a", None);
        assert_eq!(run(&m).unwrap_err().code, ErrorCode::ManifestInvalid);
        let mut m = base();
        m.check(json!({"Type": 0, "Loc": 9, "File": "a"}));
        assert_eq!(run(&m).unwrap_err().code, ErrorCode::ManifestInvalid);
        let mut m = base();
        m.file_attr("%DESTINATION%\\x", "yesterday");
        assert_eq!(run(&m).unwrap_err().code, ErrorCode::ManifestInvalid);
    }

    #[test]
    fn tokens_and_verbs() {
        assert_eq!(variable_tokens("%A%\\b\\%c_1%"), ["A", "C_1"]);
        assert_eq!(variable_tokens("50% off %X%"), ["X"]);
        assert!(variable_tokens("no tokens").is_empty());
        assert_eq!(post_command_verb("cmd.exe /C DEL \"x\""), "del");
        assert_eq!(post_command_verb("cmd /c ren a b"), "ren");
        assert_eq!(post_command_verb("  "), "");
    }
}
