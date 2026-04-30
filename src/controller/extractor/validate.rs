use crate::validation::ValidatorTrait;
use axum::extract::{Form, FromRequest, Json, Query, Request};
use serde::de::DeserializeOwned;

use crate::Error;

/// Axum middleware for validating JSON request bodies
///
/// This module provides extractors for validating JSON request bodies, form
/// data, path parameters, and query parameters using the `validator` crate.
/// Each extractor supports both detailed validation error messages
/// (`WithMessage` variants) and simplified error responses.
///
/// # Example:
///
/// ```
/// use axum::{routing::post, Router};
/// use serde::{Deserialize, Serialize};
/// use loco_rs::controller::extractor::validate::JsonValidateWithMessage;
/// use validator::Validate;
///
/// #[derive(Serialize, Deserialize, Validate)]
/// struct User {
///     #[validate(length(min = 3, message = "username must be at least 3 characters"))]
///     username: String,
///     #[validate(email(message = "email must be valid"))]
///     email: String,
/// }
///
/// async fn create_user(JsonValidateWithMessage(user): JsonValidateWithMessage<User>) -> String {
///     format!("User created: {}, {}", user.username, user.email)
/// }
///
/// fn app() -> Router {
///     Router::new()
///         .route("/users", post(create_user))
/// }
/// ```

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonValidateWithMessage<T>(pub T);

impl<T, S> FromRequest<S> for JsonValidateWithMessage<T>
where
    T: DeserializeOwned + ValidatorTrait,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state).await?;
        value.validate().map_err(Error::Validation)?;
        Ok(Self(value))
    }
}

/// Axum middleware for validating form data
///
/// # Example:
///
/// ```
/// use axum::{routing::post, Router};
/// use serde::{Deserialize, Serialize};
/// use loco_rs::controller::extractor::validate::FormValidateWithMessage;
/// use validator::Validate;
///
/// #[derive(Serialize, Deserialize, Validate)]
/// struct User {
///     #[validate(length(min = 3, message = "username must be at least 3 characters"))]
///     username: String,
///     #[validate(email(message = "email must be valid"))]
///     email: String,
/// }
///
/// async fn create_user(FormValidateWithMessage(user): FormValidateWithMessage<User>) -> String {
///     format!("User created: {}, {}", user.username, user.email)
/// }
///
/// fn app() -> Router {
///     Router::new()
///         .route("/users", post(create_user))
/// }
/// ```

#[derive(Debug, Clone, Copy, Default)]
pub struct FormValidateWithMessage<T>(pub T);

impl<T, S> FromRequest<S> for FormValidateWithMessage<T>
where
    T: DeserializeOwned + ValidatorTrait,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Form(value) = Form::<T>::from_request(req, state).await?;
        value.validate().map_err(Error::Validation)?;
        Ok(Self(value))
    }
}

/// Axum middleware for validating JSON request bodies with simplified error
/// handling
///
/// # Example:
///
/// ```
/// use axum::{routing::post, Router};
/// use serde::{Deserialize, Serialize};
/// use loco_rs::controller::extractor::validate::JsonValidate;
/// use validator::Validate;
///
/// #[derive(Serialize, Deserialize, Validate)]
/// struct User {
///     #[validate(length(min = 3, message = "username must be at least 3 characters"))]
///     username: String,
///     #[validate(email(message = "email must be valid"))]
///     email: String,
/// }
///
/// async fn create_user(JsonValidate(user): JsonValidate<User>) -> String {
///     format!("User created: {}, {}", user.username, user.email)
/// }
///
/// fn app() -> Router {
///     Router::new()
///         .route("/users", post(create_user))
/// }
/// ```

#[derive(Debug, Clone, Copy, Default)]
pub struct JsonValidate<T>(pub T);

impl<T, S> FromRequest<S> for JsonValidate<T>
where
    T: DeserializeOwned + ValidatorTrait,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state).await?;
        value.validate().map_err(|err| {
            tracing::debug!(err = ?err, "request validation error occurred");
            Error::BadRequest(String::new())
        })?;
        Ok(Self(value))
    }
}

/// Axum middleware for validating form data with simplified error handling
///
/// # Example:
///
/// ```
/// use axum::{routing::post, Router};
/// use serde::{Deserialize, Serialize};
/// use loco_rs::controller::extractor::validate::FormValidate;
/// use validator::Validate;
///
/// #[derive(Serialize, Deserialize, Validate)]
/// struct User {
///     #[validate(length(min = 3, message = "username must be at least 3 characters"))]
///     username: String,
///     #[validate(email(message = "email must be valid"))]
///     email: String,
/// }
///
/// async fn create_user(FormValidate(user): FormValidate<User>) -> String {
///     format!("User created: {}, {}", user.username, user.email)
/// }
///
/// fn app() -> Router {
///     Router::new()
///         .route("/users", post(create_user))
/// }
/// ```

#[derive(Debug, Clone, Copy, Default)]
pub struct FormValidate<T>(pub T);

impl<T, S> FromRequest<S> for FormValidate<T>
where
    T: DeserializeOwned + ValidatorTrait,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Form(value) = Form::<T>::from_request(req, state).await?;
        value.validate().map_err(|err| {
            tracing::debug!(err = ?err, "request validation error occurred");
            Error::BadRequest(String::new())
        })?;
        Ok(Self(value))
    }
}

/// Axum middleware for validating query parameters
///
/// # Example:
///
/// ```
/// use axum::{routing::get, Router};
/// use serde::{Deserialize, Serialize};
/// use loco_rs::controller::extractor::validate::QueryValidateWithMessage;
/// use validator::Validate;
///
/// #[derive(Serialize, Deserialize, Validate)]
/// struct UserQuery {
///     #[validate(length(min = 3, message = "username must be at least 3 characters"))]
///     username: String,
///     #[validate(email(message = "email must be valid"))]
///     email: String,
/// }
///
/// async fn get_user(QueryValidateWithMessage(params): QueryValidateWithMessage<UserQuery>) -> String {
///     format!("User: {}, Email: {}", params.username, params.email)
/// }
///
/// fn app() -> Router {
///     Router::new()
///         .route("/users", get(get_user))
/// }
/// ```

#[derive(Debug, Clone, Copy, Default)]
pub struct QueryValidateWithMessage<T>(pub T);

impl<T, S> FromRequest<S> for QueryValidateWithMessage<T>
where
    T: DeserializeOwned + ValidatorTrait,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request(req, state)
            .await
            .map_err(|rejection| Error::BadRequest(format!("Invalid query string: {rejection}")))?;
        value.validate().map_err(Error::Validation)?;
        Ok(Self(value))
    }
}

/// Axum middleware for validating query parameters with simplified error
/// handling
///
/// # Example:
///
/// ```
/// use axum::{routing::get, Router};
/// use serde::{Deserialize, Serialize};
/// use loco_rs::controller::extractor::validate::QueryValidate;
/// use validator::Validate;
///
/// #[derive(Serialize, Deserialize, Validate)]
/// struct UserQuery {
///     #[validate(length(min = 3, message = "username must be at least 3 characters"))]
///     username: String,
///     #[validate(email(message = "email must be valid"))]
///     email: String,
/// }
///
/// async fn get_user(QueryValidate(params): QueryValidate<UserQuery>) -> String {
///     format!("User: {}, Email: {}", params.username, params.email)
/// }
///
/// fn app() -> Router {
///     Router::new()
///         .route("/users", get(get_user))
/// }
/// ```

#[derive(Debug, Clone, Copy, Default)]
pub struct QueryValidate<T>(pub T);

impl<T, S> FromRequest<S> for QueryValidate<T>
where
    T: DeserializeOwned + ValidatorTrait,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request(req, state)
            .await
            .map_err(|rejection| Error::BadRequest(format!("Invalid query string: {rejection}")))?;
        value.validate().map_err(|err| {
            tracing::debug!(err = ?err, "query validation error occurred");
            Error::BadRequest(String::new())
        })?;
        Ok(Self(value))
    }
}

