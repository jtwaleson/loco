//! This module contains utility functions for generating HTTP responses that
//! are commonly used in web applications. These functions simplify the process
//! of creating responses with various data types.
//!
//! # Example:
//!
//! This example illustrates how to construct a JSON-formatted response using a
//! Rust struct.
//!
//! ```rust
//! use loco_rs::prelude::*;
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! pub struct Health {
//!     pub ok: bool,
//! }
//!
//! async fn ping() -> Result<Response> {
//!    format::json(Health { ok: true })
//! }
//! ```
use std::convert::TryInto;

use axum::{
    body::Body,
    http::{header, response::Builder, HeaderName, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::Cookie;
use bytes::{BufMut, BytesMut};
use serde::Serialize;
use serde_json::json;

use crate::{
    controller::Json,
    Result,
};

/// Returns an empty response.
///
/// # Example:
///
/// This example illustrates how to return an empty response.
/// ```rust
/// use loco_rs::prelude::*;
///
/// async fn endpoint() -> Result<Response> {
///    format::empty()
/// }
/// ```
///
/// # Errors
///
/// Currently this function doesn't return any error. this is for feature
/// functionality
pub fn empty() -> Result<Response> {
    Ok(().into_response())
}

/// Returns a response containing the provided text.
///
/// # Example:
///
/// This example illustrates how to return an text response.
/// ```rust
/// use loco_rs::prelude::*;
///
/// async fn endpoint() -> Result<Response> {
///    format::text("MESSAGE-RESPONSE")
/// }
/// ```
///
/// # Errors
///
/// Currently this function doesn't return any error. this is for feature
/// functionality
pub fn text(t: &str) -> Result<Response> {
    Ok(t.to_string().into_response())
}

/// Returns a JSON response containing the provided data.
///
/// # Example:
///
/// This example illustrates how to construct a JSON-formatted response using a
/// Rust struct.
///
/// ```rust
/// use loco_rs::prelude::*;
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// pub struct Health {
///     pub ok: bool,
/// }
///
/// async fn endpoint() -> Result<Response> {
///    format::json(Health { ok: true })
/// }
/// ```
///
/// # Errors
///
/// Currently this function doesn't return any error. this is for feature
/// functionality
pub fn json<T: Serialize>(t: T) -> Result<Response> {
    Ok(Json(t).into_response())
}

/// Respond with empty json (`{}`)
///
/// # Errors
///
/// This function will return an error if serde fails
pub fn empty_json() -> Result<Response> {
    json(json!({}))
}

/// Returns an HTML response
///
/// # Example:
///
/// ```rust
/// use loco_rs::prelude::*;
///
/// async fn endpoint() -> Result<Response> {
///    format::html("hello, world")
/// }
/// ```
///
/// # Errors
///
/// Currently this function doesn't return any error. this is for feature
/// functionality
pub fn html(content: &str) -> Result<Response> {
    Ok(Html(content.to_string()).into_response())
}

/// Returns a YAML response
///
/// # Example:
///
/// ```rust
/// use loco_rs::prelude::*;
///
/// pub async fn openapi_spec_yaml() -> Result<Response> {
///     format::yaml("openapi: 3.1.0\ninfo:\n  title: Loco Demo\n  ")
/// }
/// ```
///
/// # Errors
///
/// Currently this function doesn't return any error. this is for feature
/// functionality
pub fn yaml(content: &str) -> Result<Response> {
    Ok(Builder::new()
        .header(header::CONTENT_TYPE, "application/yaml")
        .body(Body::from(content.to_string()))?
        .into_response())
}

/// Returns an redirect response
///
/// # Example:
///
/// ```rust
/// use loco_rs::prelude::*;
///
/// async fn login() -> Result<Response> {
///    format::redirect("/dashboard")
/// }
/// ```
///
/// # Errors
///
/// Currently this function doesn't return any error. this is for feature
/// functionality
pub fn redirect(to: &str) -> Result<Response> {
    Ok(Redirect::to(to).into_response())
}

#[derive(Debug)]
pub struct RenderBuilder {
    response: Builder,
}

impl RenderBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            response: Builder::new().status(StatusCode::OK),
        }
    }

    /// Get an Axum response builder (escape hatch, leaving this builder)
    #[must_use]
    pub fn response(self) -> Builder {
        self.response
    }

    /// Add a status code
    #[must_use]
    pub fn status<T>(self, status: T) -> Self
    where
        StatusCode: TryFrom<T>,
        <StatusCode as TryFrom<T>>::Error: Into<axum::http::Error>,
    {
        Self {
            response: self.response.status(status),
        }
    }

    /// Add a single header
    #[must_use]
    pub fn header<K, V>(self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<axum::http::Error>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<axum::http::Error>,
    {
        Self {
            response: self.response.header(key, value),
        }
    }

    /// Add an etag
    ///
    /// # Errors
    ///
    /// This function will return an error if provided etag value is illegal
    /// (not visible ASCII)
    pub fn etag(self, etag: &str) -> Result<Self> {
        Ok(Self {
            response: self
                .response
                .header(header::ETAG, HeaderValue::from_str(etag)?),
        })
    }

    /// Add a collection of cookies to the response
    ///
    /// # Errors
    /// Returns error if cookie values are illegal
    pub fn cookies(self, cookies: &[Cookie<'_>]) -> Result<Self> {
        let mut res = self.response;
        for cookie in cookies {
            let header_value = cookie.encoded().to_string().parse::<HeaderValue>()?;
            res = res.header(header::SET_COOKIE, header_value);
        }
        Ok(Self { response: res })
    }

    /// Finalize and return a text response
    ///
    /// # Errors
    ///
    /// This function will return an error if IO fails
    pub fn text(self, content: &str) -> Result<Response> {
        Ok(self
            .response
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )
            .body(Body::from(content.to_string()))?)
    }

    /// Finalize and return an empty response
    ///
    /// # Errors
    ///
    /// This function will return an error if IO fails
    pub fn empty(self) -> Result<Response> {
        Ok(self.response.body(Body::empty())?)
    }

    /// Finalize and return a HTML response
    ///
    /// # Errors
    ///
    /// This function will return an error if IO fails
    pub fn html(self, content: &str) -> Result<Response> {
        Ok(self
            .response
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )
            .body(Body::from(content.to_string()))?)
    }

    /// Finalize and return a JSON response
    ///
    /// # Errors
    ///
    /// This function will return an error if IO fails
    pub fn json<T>(self, item: T) -> Result<Response>
    where
        T: Serialize,
    {
        let mut buf = BytesMut::with_capacity(128).writer();
        serde_json::to_writer(&mut buf, &item)?;
        let body = Body::from(buf.into_inner().freeze());
        Ok(self
            .response
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )
            .body(body)?)
    }

    /// Finalize and redirect request
    ///
    /// # Errors
    ///
    /// This function will return an error if IO fails
    pub fn redirect(self, to: &str) -> Result<Response> {
        self.redirect_with_header_key(header::LOCATION, to)
    }

    /// Finalizes the HTTP response and redirects to a specified location using
    /// a dynamic header key.
    ///
    /// # Errors
    ///
    /// This function will return an error if IO fails
    pub fn redirect_with_header_key<K>(self, key: K, to: &str) -> Result<Response>
    where
        K: TryInto<HeaderName>,
        <K as TryInto<HeaderName>>::Error: Into<axum::http::Error>,
    {
        Ok(self
            .response
            .status(StatusCode::SEE_OTHER)
            .header(key, to)
            .body(Body::empty())?)
    }
}

impl Default for RenderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub fn render() -> RenderBuilder {
    RenderBuilder::new()
}

