use serde::{Deserialize, Serialize};
use std::fmt;

/// Schema format version for ReForge portable artifacts.
///
/// Phase 1 current format is `1.0.0`. Older fixtures may use `0.9.0`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(transparent)]
pub struct SchemaVersion(String);

impl SchemaVersion {
    pub const V1_0_0: &'static str = "1.0.0";
    pub const V0_9_0: &'static str = "0.9.0";

    pub fn new(value: impl Into<String>) -> Result<Self, SchemaVersionError> {
        let value = value.into();
        let parsed = semver::Version::parse(&value)
            .map_err(|_| SchemaVersionError::Invalid(value.clone()))?;
        if parsed.major > 1 {
            return Err(SchemaVersionError::UnsupportedFuture(value));
        }
        Ok(Self(value))
    }

    pub fn current() -> Self {
        Self(Self::V1_0_0.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse_semver(&self) -> semver::Version {
        semver::Version::parse(&self.0).expect("validated schema version")
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self::current()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SchemaVersionError {
    #[error("invalid schema version: {0}")]
    Invalid(String),
    #[error("unsupported future schema version: {0}")]
    UnsupportedFuture(String),
}

/// Package semantic version wrapper.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackageVersion(String);

impl PackageVersion {
    pub fn new(value: impl Into<String>) -> Result<Self, PackageVersionError> {
        let value = value.into();
        semver::Version::parse(&value).map_err(|_| PackageVersionError::Invalid(value.clone()))?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(&self) -> semver::Version {
        semver::Version::parse(&self.0).expect("validated package version")
    }
}

impl fmt::Display for PackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PackageVersionError {
    #[error("invalid package semantic version: {0}")]
    Invalid(String),
}

/// Version constraint string using cargo/semver syntax (e.g. `^1.2`, `>=0.3.0,<0.4.0`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionReq(String);

impl VersionReq {
    pub fn new(value: impl Into<String>) -> Result<Self, VersionReqError> {
        let value = value.into();
        semver::VersionReq::parse(&value).map_err(|_| VersionReqError::Invalid(value.clone()))?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches(&self, version: &PackageVersion) -> bool {
        let req = semver::VersionReq::parse(&self.0).expect("validated");
        req.matches(&version.parse())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VersionReqError {
    #[error("invalid version requirement: {0}")]
    Invalid(String),
}
