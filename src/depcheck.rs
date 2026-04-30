//! Cargo.lock dependency version checks (used by unit tests; previously by `doctor`).
#![allow(dead_code)]

use std::collections::HashMap;

use semver::{Version, VersionReq};
use thiserror::Error;

use crate::cargo_config::CargoConfig;

#[derive(Debug, PartialEq, Eq, Ord, PartialOrd)]
pub enum VersionStatus {
    NotFound,
    Invalid {
        version: String,
        min_version: String,
    },
    Ok(String),
}

#[derive(Debug, PartialEq, Eq, Ord, PartialOrd)]
pub struct CrateStatus {
    pub crate_name: String,
    pub status: VersionStatus,
}

#[derive(Error, Debug)]
pub enum VersionCheckError {
    #[error("Failed to parse Cargo.lock: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Error with crate {crate_name}: {msg}")]
    CrateError { crate_name: String, msg: String },
}

pub type Result<T> = std::result::Result<T, VersionCheckError>;

pub fn check_crate_versions(
    lock_file: &CargoConfig,
    min_versions: HashMap<&str, &str>,
) -> Result<Vec<CrateStatus>> {
    let packages = lock_file
        .get_package_array()
        .map_err(|e| VersionCheckError::ParseError(serde::de::Error::custom(e.to_string())))?;

    let mut results = Vec::new();

    for (crate_name, min_version) in min_versions {
        let min_version_req =
            VersionReq::parse(min_version).map_err(|_| VersionCheckError::CrateError {
                crate_name: crate_name.to_string(),
                msg: format!("Invalid minimum version format: {min_version}"),
            })?;

        let mut found = false;
        for package in packages {
            if let Some(name) = package.get("name").and_then(|v| v.as_str()) {
                if name == crate_name {
                    found = true;
                    let version_str =
                        package
                            .get("version")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| VersionCheckError::CrateError {
                                crate_name: crate_name.to_string(),
                                msg: "Invalid version format in Cargo.lock".to_string(),
                            })?;

                    let version =
                        Version::parse(version_str).map_err(|_| VersionCheckError::CrateError {
                            crate_name: crate_name.to_string(),
                            msg: format!("Invalid version format in Cargo.lock: {version_str}"),
                        })?;

                    let status = if min_version_req.matches(&version) {
                        VersionStatus::Ok(version.to_string())
                    } else {
                        VersionStatus::Invalid {
                            version: version.to_string(),
                            min_version: min_version.to_string(),
                        }
                    };
                    results.push(CrateStatus {
                        crate_name: crate_name.to_string(),
                        status,
                    });
                    break;
                }
            }
        }

        if !found {
            results.push(CrateStatus {
                crate_name: crate_name.to_string(),
                status: VersionStatus::NotFound,
            });
        }
    }

    Ok(results)
}

