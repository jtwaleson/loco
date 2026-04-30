//! Cargo.toml configuration reader module
//!
//! This module provides functionality to read and parse Cargo.toml files,
//! with specific support for accessing database entity configuration
//! under the `[package.metadata.db.entity]` section.

use crate::errors::Error;
use crate::Result as AppResult;
use std::path::Path;
use toml::Table;

/// Represents a parsed Cargo.toml configuration
///
/// This struct holds the parsed TOML data from a Cargo.toml file
/// and provides methods to access specific sections of the configuration.
pub struct CargoConfig {
    toml: Table,
}

impl CargoConfig {
    /// Creates a new [`CargoConfig`] by reading the Cargo.toml file from the current directory
    ///
    /// # Errors
    /// * If the Cargo.toml file cannot be read
    /// * If the file contains invalid TOML
    pub fn from_current_dir() -> AppResult<Self> {
        Self::from_path("Cargo.toml")
    }

    /// Creates a new [`CargoConfig`] by reading the Cargo.lock file from the current directory
    ///
    /// # Errors
    /// * If the Cargo.lock file cannot be read
    /// * If the file contains invalid TOML
    pub fn lock_from_current_dir() -> AppResult<Self> {
        Self::from_path("Cargo.lock")
    }

    /// Creates a new [`CargoConfig`] by reading and parsing a TOML file from the specified path
    ///
    /// # Errors
    /// * If the file cannot be read
    /// * If the file contains invalid TOML
    pub fn from_path(path: impl AsRef<Path>) -> AppResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::Message(format!("Failed to read Cargo.toml: {e}")))?;

        let toml = content
            .parse::<Table>()
            .map_err(|e| Error::Message(format!("Failed to parse Cargo.toml: {e}")))?;

        Ok(Self { toml })
    }

    /// Retrieves the database entity configuration from the Cargo.toml
    ///
    /// Looks for configuration under the `[package.metadata.db.entity]` section.
    #[must_use]
    pub fn get_db_entities(&self) -> Option<&Table> {
        self.toml
            .get("package")
            .and_then(|p| p.as_table())
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.as_table())
            .and_then(|m| m.get("db"))
            .and_then(|d| d.as_table())
            .and_then(|d| d.get("entity"))
            .and_then(|e| e.as_table())
    }

    /// Gets the package array from Cargo.lock
    ///
    /// # Errors
    /// Returns an error if the package array is missing or invalid
    pub fn get_package_array(&self) -> AppResult<&[toml::Value]> {
        self.toml
            .get("package")
            .and_then(|v| v.as_array())
            .map(std::vec::Vec::as_slice)
            .ok_or_else(|| Error::Message("Missing package array in Cargo.lock".to_string()))
    }
}

