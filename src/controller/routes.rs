use std::convert::Infallible;

use axum::{extract::Request, response::IntoResponse, routing::Route};
use tower::{Layer, Service};

use super::describe;
use crate::app::AppContext;
#[derive(Clone, Default, Debug)]
pub struct Routes {
    pub prefix: Option<String>,
    pub handlers: Vec<Handler>,
    // pub version: Option<String>,
}

#[derive(Clone, Default, Debug)]
pub struct Handler {
    pub uri: String,
    pub method: axum::routing::MethodRouter<AppContext>,
    pub actions: Vec<axum::http::Method>,
}

impl Routes {
    /// Creates a new [`Routes`] instance with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a prefix for the routes. this prefix will be a prefix for all the
    /// routes.
    ///
    /// # Example
    ///
    /// In the following example the we are adding `status`  as a prefix to the
    /// _ping endpoint HOST/status/_ping.
    ///
    /// ```rust
    /// use loco_rs::prelude::*;
    /// use serde::Serialize;;
    ///
    /// #[derive(Serialize)]
    /// struct Health {
    ///    pub ok: bool,
    /// }
    ///
    /// async fn ping() -> Result<Response> {
    ///     format::json(Health { ok: true })
    /// }
    /// Routes::at("status").add("/_ping", get(ping));
    ///    
    /// ````
    #[must_use]
    pub fn at(prefix: &str) -> Self {
        Self {
            prefix: Some(prefix.to_string()),
            ..Self::default()
        }
    }

    /// Adding new router
    ///
    /// # Example
    ///
    /// This example preset how to add a get endpoint int the Router.
    ///
    /// ```rust
    /// use loco_rs::prelude::*;
    /// use serde::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct Health {
    ///    pub ok: bool,
    /// }
    ///
    /// async fn ping() -> Result<Response> {
    ///     format::json(Health { ok: true })
    /// }
    /// Routes::new().add("/_ping", get(ping));
    /// ````
    #[must_use]
    pub fn add(mut self, uri: &str, method: axum::routing::MethodRouter<AppContext>) -> Self {
        describe::method_action(&method);
        self.handlers.push(Handler {
            uri: uri.to_owned(),
            actions: describe::method_action(&method),
            method,
        });
        self
    }

    /// Merge another Routes instance into this one.
    ///
    /// This method allows you to combine multiple Routes instances into a single
    /// Routes struct. All handlers from the other Routes will be added to this one.
    /// This is useful for collecting routes from different controllers before
    /// nesting them under a common prefix.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loco_rs::prelude::*;
    /// use axum::routing::{get, post};
    ///
    /// async fn list_users() -> Result<Response> {
    ///     format::json("users list")
    /// }
    ///
    /// async fn create_user() -> Result<Response> {
    ///     format::json("user created")
    /// }
    ///
    /// async fn list_products() -> Result<Response> {
    ///     format::json("products list")
    /// }
    ///
    /// async fn create_product() -> Result<Response> {
    ///     format::json("product created")
    /// }
    ///
    /// // Create separate route groups
    /// let user_routes = Routes::new()
    ///     .add("/users", get(list_users))
    ///     .add("/users", post(create_user));
    ///
    /// let product_routes = Routes::new()
    ///     .add("/products", get(list_products))
    ///     .add("/products", post(create_product));
    ///
    /// // Merge them into a single Routes instance
    /// let api_routes = Routes::new()
    ///     .merge(user_routes)
    ///     .merge(product_routes);
    ///
    /// // Now nest the combined routes under /api
    /// let app_routes = Routes::new()
    ///     .add("/health", get(|| async { "ok" }))
    ///     .nest("/api", api_routes);
    ///
    /// // This will result in routes:
    /// // - GET /health
    /// // - GET /api/users
    /// // - POST /api/users
    /// // - GET /api/products
    /// // - POST /api/products
    /// ```
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        // Extend the handlers vector with all handlers from the other Routes
        self.handlers.extend(other.handlers);
        self
    }

    /// Merge multiple Routes instances into this one.
    ///
    /// This is a convenience method that allows you to merge multiple Routes
    /// instances at once, which is particularly useful when setting up `AppRoutes`
    /// and you want to collect routes from different controllers before nesting them.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loco_rs::prelude::*;
    /// use axum::routing::{get, post};
    ///
    /// async fn list_users() -> Result<Response> {
    ///     format::json("users list")
    /// }
    ///
    /// async fn list_products() -> Result<Response> {
    ///     format::json("products list")
    /// }
    ///
    /// async fn list_orders() -> Result<Response> {
    ///     format::json("orders list")
    /// }
    ///
    /// // Create separate route groups from different controllers
    /// let user_routes = Routes::new().add("/users", get(list_users));
    /// let product_routes = Routes::new().add("/products", get(list_products));
    /// let order_routes = Routes::new().add("/orders", get(list_orders));
    ///
    /// // Merge all of them at once
    /// let api_routes = Routes::new().merge_all(vec![user_routes, product_routes, order_routes]);
    ///
    /// // Now nest the combined routes under /api
    /// let app_routes = Routes::new()
    ///     .add("/health", get(|| async { "ok" }))
    ///     .nest("/api", api_routes);
    ///
    /// // This will result in routes:
    /// // - GET /health
    /// // - GET /api/users
    /// // - GET /api/products
    /// // - GET /api/orders
    /// ```
    #[must_use]
    pub fn merge_all(mut self, others: Vec<Self>) -> Self {
        // Extend the handlers vector with all handlers from all Routes
        for other in others {
            self.handlers.extend(other.handlers);
        }
        self
    }

    /// Set a prefix for the routes. this prefix will be a prefix for all the
    /// routes.
    ///
    /// # Example
    ///
    /// In the following example the we are adding `status`  as a prefix to the
    /// _ping endpoint HOST/status/_ping.
    ///
    /// ```rust
    /// use loco_rs::prelude::*;
    /// use serde::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct Health {
    ///    pub ok: bool,
    /// }
    ///
    /// async fn ping() -> Result<Response> {
    ///     format::json(Health { ok: true })
    /// }
    /// Routes::new().prefix("status").add("/_ping", get(ping));
    /// ````
    #[must_use]
    pub fn prefix(mut self, uri: &str) -> Self {
        self.prefix = Some(uri.to_owned());
        self
    }

    /// Set a layer for the routes. this layer will be a layer for all the
    /// routes.
    ///
    /// # Example
    ///
    /// In the following example, we are adding a layer to the routes.
    ///
    /// ```rust
    /// use loco_rs::prelude::*;
    /// use tower::{Layer, Service};
    /// use tower_http::trace::TraceLayer;
    /// async fn ping() -> Result<Response> {
    ///     format::json("Ok")
    /// }
    /// Routes::new().prefix("status").add("/_ping", get(ping)).layer(TraceLayer::new_for_http());
    /// ```
    #[allow(clippy::needless_pass_by_value)]
    #[must_use]
    pub fn layer<L>(self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        Self {
            prefix: self.prefix,
            handlers: self
                .handlers
                .iter()
                .map(|handler| Handler {
                    uri: handler.uri.clone(),
                    actions: handler.actions.clone(),
                    method: handler.method.clone().layer(layer.clone()),
                })
                .collect(),
        }
    }

    /// Nest another Routes instance under a prefix path.
    ///
    /// This method allows you to nest a group of routes under a specific path prefix,
    /// similar to Axum's `nest` method. The nested routes will have their URIs
    /// prefixed with the given path.
    ///
    /// # Example
    ///
    /// ```rust
    /// use loco_rs::prelude::*;
    /// use axum::routing::{get, post, delete, patch};
    ///
    /// // Define user-related handlers
    /// async fn list_users() -> Result<Response> {
    ///     format::json("users list")
    /// }
    ///
    /// async fn get_user() -> Result<Response> {
    ///     format::json("user detail")
    /// }
    ///
    /// async fn create_user() -> Result<Response> {
    ///     format::json("user created")
    /// }
    ///
    /// async fn update_user() -> Result<Response> {
    ///     format::json("user updated")
    /// }
    ///
    /// async fn delete_user() -> Result<Response> {
    ///     format::json("user deleted")
    /// }
    ///
    /// // Create API routes for users
    /// let user_routes = Routes::new()
    ///     .add("/users", get(list_users))
    ///     .add("/users", post(create_user))
    ///     .add("/users/{id}", get(get_user))
    ///     .add("/users/{id}", patch(update_user))
    ///     .add("/users/{id}", delete(delete_user));
    ///
    /// // Create the main application routes
    /// let app_routes = Routes::new()
    ///     .add("/health", get(|| async { "ok" }))
    ///     .nest("/api/v1", user_routes);
    ///
    /// // This will result in routes:
    /// // - GET /health
    /// // - GET /api/v1/users
    /// // - POST /api/v1/users
    /// // - GET /api/v1/users/{id}
    /// // - PATCH /api/v1/users/{id}
    /// // - DELETE /api/v1/users/{id}
    /// ```
    #[must_use]
    pub fn nest(mut self, path: &str, nested_routes: Self) -> Self {
        // Normalize the path to ensure it starts with / and doesn't end with /
        let mut normalized_path = path.to_string();
        if !normalized_path.starts_with('/') {
            normalized_path.insert(0, '/');
        }
        if normalized_path.ends_with('/') && normalized_path != "/" {
            normalized_path.pop();
        }

        // Process each handler from the nested routes
        for handler in nested_routes.handlers {
            // Combine the path prefix with the handler's URI
            let combined_uri = if handler.uri == "/" {
                normalized_path.clone()
            } else {
                format!("{}{}", normalized_path, handler.uri)
            };

            // Create a new handler with the combined URI
            let new_handler = Handler {
                uri: combined_uri,
                method: handler.method,
                actions: handler.actions,
            };

            self.handlers.push(new_handler);
        }

        self
    }
}

