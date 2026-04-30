pub use async_trait::async_trait;
pub use axum::{
    debug_handler,
    extract::{Form, Multipart, Path, Query, State},
    response::{IntoResponse, Response},
    routing::{delete, get, head, options, patch, post, put, trace},
};
pub use axum_extra::extract::cookie;
pub use chrono::NaiveDateTime as DateTime;
// some types required for controller generators
#[cfg(feature = "with-db")]
pub use sea_orm::prelude::{Date, DateTimeUtc, DateTimeWithTimeZone, Decimal, Uuid};
#[cfg(feature = "with-db")]
pub use sea_orm::{
    ActiveModelBehavior, ActiveModelTrait, ActiveValue, ColumnTrait, ConnectionTrait,
    DatabaseConnection, DbErr, EntityTrait, IntoActiveModel, ModelTrait, QueryFilter, Set,
    TransactionTrait,
};
// sugar for controllers to use `data!({"item": ..})` instead of `json!`
pub use serde_json::json as data;

#[cfg(feature = "auth_jwt")]
pub use crate::controller::extractor::auth;
pub use crate::controller::extractor::{
    respond_to::{Format, RespondTo},
    shared_store::SharedStore,
    validate::{JsonValidate, JsonValidateWithMessage},
};
#[cfg(feature = "with-db")]
pub use crate::model::{Authenticable, ModelError, ModelResult};
pub use crate::{
    app::{AppContext, Initializer},
    bgworker::{BackgroundWorker, Queue},
    controller::{
        bad_request, format,
        not_found, unauthorized,
        Json, Routes,
    },
    errors::Error,
    task::{self, Task, TaskInfo},
    validation::{self, Validatable, ValidatorTrait},
    Result,
};
pub use validator::Validate;
