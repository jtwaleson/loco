//! Static Assets Middleware.
//!
//! This middleware serves static files (e.g., images, CSS, JS) from a specified
//! folder to the client. It also allows configuration of a fallback file to
//! serve in case a requested file is not found. Additionally, it can serve
//! precompressed files if enabled via the configuration.
//!
//! The middleware supports path-based cache control through regex patterns.
//! You can configure `regex_cache` with a list of regex patterns and cache
//! control headers. The first regex that matches the request path will have
//! its cache_control header applied. If no regex matches, the default
//! `cache_control` value is used (if configured).
//!
//! The middleware checks if the specified folder and fallback file exist, and
//! if either is missing, it returns an error. If the files exist, the
//! middleware is added to the router to serve static files.

use std::path::PathBuf;

use axum::{
    extract::Request,
    http::header::{HeaderValue, CACHE_CONTROL},
    middleware::Next,
    Router as AXRouter,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};

use crate::{app::AppContext, controller::middleware::MiddlewareLayer, Error, Result};

/// Static asset middleware configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StaticAssets {
    #[serde(default)]
    pub enable: bool,
    /// Check that assets must exist on disk
    #[serde(default = "default_must_exist")]
    pub must_exist: bool,
    /// Assets location
    #[serde(default = "default_folder_config")]
    pub folder: FolderConfig,
    /// Fallback page for a case when no asset exists. Useful for SPA
    /// (single page app) where routes are virtual.
    #[serde(default = "default_fallback")]
    pub fallback: PathBuf,
    /// Enable `precompressed_gzip`
    #[serde(default = "default_precompressed")]
    pub precompressed: bool,
    /// Cache control header value for static assets (e.g., "max-age=31536000")
    /// This is used as the default cache control when no regex matches.
    pub cache_control: Option<String>,
    /// List of regex patterns with cache control headers.
    /// The first regex that matches the request path will have its cache_control applied.
    #[serde(default)]
    pub regex_cache: Vec<RegexCacheRule>,
}

impl Default for StaticAssets {
    fn default() -> Self {
        serde_json::from_value(json!({})).unwrap()
    }
}

fn default_must_exist() -> bool {
    true
}

fn default_precompressed() -> bool {
    false
}

fn default_fallback() -> PathBuf {
    PathBuf::from("assets").join("static").join("404.html")
}

fn default_folder_config() -> FolderConfig {
    FolderConfig {
        uri: "/static".to_string(),
        path: PathBuf::from("assets/static"),
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct FolderConfig {
    /// Uri for the assets
    pub uri: String,
    /// Path for the assets
    pub path: PathBuf,
}

/// Regex cache rule for path-based cache control
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegexCacheRule {
    /// Regex pattern to match against the request path
    pub pattern: String,
    /// Cache control header value to apply when pattern matches
    pub cache_control: String,
}

/// Compiled regex cache rule for efficient matching
#[derive(Clone)]
struct CompiledRegexCacheRule {
    regex: Regex,
    cache_control: HeaderValue,
}

// Implement the MiddlewareTrait for your Middleware struct
impl MiddlewareLayer for StaticAssets {
    /// Returns the name of the middleware.
    fn name(&self) -> &'static str {
        "static"
    }

    /// Checks if the static assets middleware is enabled.
    fn is_enabled(&self) -> bool {
        self.enable
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }

    /// Applies the static assets middleware to the application router.
    ///
    /// This method wraps the provided [`AXRouter`] with a service to serve
    /// static files from the folder specified in the configuration. It will
    /// serve a fallback file if the requested file is not found, and can
    /// also serve precompressed (gzip) files if enabled.
    ///
    /// Before applying, it checks if the folder and fallback file exist. If
    /// either is missing, it returns an error.
    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        if self.must_exist && (!&self.folder.path.exists() || !&self.fallback.exists()) {
            return Err(Error::Message(format!(
                "one of the static path are not found, Folder `{}` fallback: `{}`",
                self.folder.path.display(),
                self.fallback.display(),
            )));
        }

        // Compile regex patterns if regex_cache is configured
        let compiled_rules: Result<Vec<CompiledRegexCacheRule>> = self
            .regex_cache
            .iter()
            .map(|rule| {
                let regex = Regex::new(&rule.pattern).map_err(|e| {
                    Error::Message(format!(
                        "invalid regex pattern '{}': {}",
                        rule.pattern, e
                    ))
                })?;
                let cache_control = HeaderValue::from_str(&rule.cache_control).map_err(|e| {
                    Error::Message(format!(
                        "invalid cache_control value '{}': {}",
                        rule.cache_control, e
                    ))
                })?;
                Ok(CompiledRegexCacheRule {
                    regex,
                    cache_control,
                })
            })
            .collect();

        let compiled_rules = compiled_rules?;
        let default_cache_control = self
            .cache_control
            .as_ref()
            .and_then(|cc| HeaderValue::from_str(cc).ok());

        let serve_dir = ServeDir::new(&self.folder.path).fallback(ServeFile::new(&self.fallback));

        let base_service = if self.precompressed {
            serve_dir.precompressed_gzip()
        } else {
            serve_dir
        };

        // Create static service with cache control middleware if needed
        let static_service = if !compiled_rules.is_empty() || default_cache_control.is_some() {
            // Use middleware to check regex patterns and apply cache control
            let rules = compiled_rules.clone();
            let default_cc = default_cache_control.clone();

            AXRouter::new()
                .fallback_service(base_service)
                .layer(axum::middleware::from_fn(move |request: Request, next: Next| {
                    let rules = rules.clone();
                    let default_cc = default_cc.clone();
                    async move {
                        // Get the request path for regex matching
                        let path = request.uri().path().to_string();
                        let mut response = next.run(request).await;

                        // Check regex patterns in order - first match wins
                        let cache_control = rules
                            .iter()
                            .find(|rule| rule.regex.is_match(&path))
                            .map(|rule| rule.cache_control.clone())
                            .or(default_cc);

                        if let Some(cc) = cache_control {
                            response.headers_mut().insert(CACHE_CONTROL, cc);
                        }

                        response
                    }
                }))
        } else {
            AXRouter::new().fallback_service(base_service)
        };

        if &self.folder.uri == "/" {
            Ok(app.fallback_service(static_service))
        } else {
            Ok(app.nest_service(&self.folder.uri, static_service))
        }
    }
}
