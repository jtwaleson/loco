#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::module_name_repetitions)]
#![doc = "loco-rs core library"]

pub use self::errors::Error;

mod banner;
pub mod bgworker;
mod depcheck;
pub mod prelude;

pub mod data;

#[cfg(feature = "with-db")]
pub mod db;
#[cfg(feature = "with-db")]
pub mod model;
mod tera;

pub mod app;
pub mod auth;
pub mod boot;
#[cfg(feature = "cli")]
pub mod cli;
pub mod config;
pub mod controller;
mod env_vars;
pub mod environment;
pub mod errors;
pub mod hash;
pub mod logger;
pub mod mailer;
pub mod task;
#[cfg(feature = "testing")]
pub mod tests_cfg;
pub mod validation;
pub use validator;
pub mod cargo_config;

/// Application results options list
pub type Result<T, E = Error> = std::result::Result<T, E>;
