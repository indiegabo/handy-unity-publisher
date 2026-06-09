//! Defines typed engine and build contracts shared across runtime planning and
//! execution flows.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

/// Reports one unsupported or malformed runtime contract value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseContractKindError {
    kind: &'static str,
    value: String,
}

impl ParseContractKindError {
    fn new(kind: &'static str, value: &str) -> Self {
        Self {
            kind,
            value: value.trim().to_owned(),
        }
    }
}

impl fmt::Display for ParseContractKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unsupported {} {:?}", self.kind, self.value)
    }
}

impl Error for ParseContractKindError {}

/// Identifies the engine contract selected for one repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EngineKind {
    #[serde(rename = "unity")]
    Unity,
    #[serde(rename = "unreal")]
    Unreal,
    #[serde(rename = "godot")]
    Godot,
    #[serde(rename = "gamemaker")]
    GameMaker,
    #[serde(rename = "defold")]
    Defold,
    #[serde(rename = "cocos-creator")]
    CocosCreator,
}

impl EngineKind {
    /// Returns the stable engine slug used across runtime persistence and JSON contracts.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unity => "unity",
            Self::Unreal => "unreal",
            Self::Godot => "godot",
            Self::GameMaker => "gamemaker",
            Self::Defold => "defold",
            Self::CocosCreator => "cocos-creator",
        }
    }

    /// Parses one persisted or user-authored engine slug into the shared contract enum.
    pub fn parse(value: &str) -> Result<Self, ParseContractKindError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "unity" => Ok(Self::Unity),
            "unreal" => Ok(Self::Unreal),
            "godot" => Ok(Self::Godot),
            "gamemaker" => Ok(Self::GameMaker),
            "defold" => Ok(Self::Defold),
            "cocos-creator" => Ok(Self::CocosCreator),
            _ => Err(ParseContractKindError::new("engine_kind", value)),
        }
    }

    /// Reports whether the runtime currently ships a real execution adapter for this engine.
    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::Unity)
    }
}

impl fmt::Display for EngineKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<&str> for EngineKind {
    type Error = ParseContractKindError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for EngineKind {
    type Error = ParseContractKindError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<EngineKind> for String {
    fn from(value: EngineKind) -> Self {
        String::from(value.as_str())
    }
}

/// Identifies the build contract selected for one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BuildKind {
    #[default]
    #[serde(rename = "player")]
    Player,
}

impl BuildKind {
    /// Returns the stable build kind slug used across runtime persistence and JSON contracts.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Player => "player",
        }
    }

    /// Parses one persisted or user-authored build kind into the shared contract enum.
    pub fn parse(value: &str) -> Result<Self, ParseContractKindError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "player" => Ok(Self::Player),
            _ => Err(ParseContractKindError::new("build_kind", value)),
        }
    }

    /// Parses one build kind and preserves the existing runtime default of `player`.
    pub fn parse_or_default(value: &str) -> Result<Self, ParseContractKindError> {
        if value.trim().is_empty() {
            Ok(Self::Player)
        } else {
            Self::parse(value)
        }
    }
}

impl fmt::Display for BuildKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<&str> for BuildKind {
    type Error = ParseContractKindError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for BuildKind {
    type Error = ParseContractKindError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<BuildKind> for String {
    fn from(value: BuildKind) -> Self {
        String::from(value.as_str())
    }
}

/// Identifies the host scheduling priority requested for one release process
/// and the child jobs that launch external tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProcessPriority {
    #[default]
    Low,
    Normal,
    High,
}

impl ProcessPriority {
    /// Returns the stable slug used across runtime persistence and JSON payloads.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }

    /// Parses one persisted or user-authored priority slug into the shared contract enum.
    pub fn parse(value: &str) -> Result<Self, ParseContractKindError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            _ => Err(ParseContractKindError::new("process_priority", value)),
        }
    }

    /// Parses one priority and preserves the runtime default of `low` for blank values.
    pub fn parse_or_default(value: &str) -> Result<Self, ParseContractKindError> {
        if value.trim().is_empty() {
            Ok(Self::Low)
        } else {
            Self::parse(value)
        }
    }

    #[cfg(windows)]
    pub const fn creation_flags(self) -> u32 {
        match self {
            Self::Low => 0x0000_4000,
            Self::Normal => 0x0000_0020,
            Self::High => 0x0000_0080,
        }
    }
}

impl fmt::Display for ProcessPriority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<&str> for ProcessPriority {
    type Error = ParseContractKindError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for ProcessPriority {
    type Error = ParseContractKindError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ProcessPriority> for String {
    fn from(value: ProcessPriority) -> Self {
        String::from(value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildKind, EngineKind, ProcessPriority};

    #[test]
    fn engine_kind_parses_supported_slugs() {
        assert_eq!(EngineKind::parse("unity").unwrap(), EngineKind::Unity);
        assert_eq!(
            EngineKind::parse("cocos-creator").unwrap(),
            EngineKind::CocosCreator
        );
    }

    #[test]
    fn build_kind_defaults_empty_values_to_player() {
        assert_eq!(BuildKind::parse_or_default("").unwrap(), BuildKind::Player);
    }

    #[test]
    fn process_priority_defaults_empty_values_to_low() {
        assert_eq!(
            ProcessPriority::parse_or_default("").unwrap(),
            ProcessPriority::Low
        );
    }

    #[test]
    fn process_priority_parses_supported_slugs() {
        assert_eq!(ProcessPriority::parse("low").unwrap(), ProcessPriority::Low);
        assert_eq!(
            ProcessPriority::parse("normal").unwrap(),
            ProcessPriority::Normal
        );
        assert_eq!(ProcessPriority::parse("high").unwrap(), ProcessPriority::High);
    }
}
