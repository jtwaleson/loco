//! # Application Bootstrapping and Logic
//! This module contains functions and structures for bootstrapping and running
//! your application.
use std::sync::Arc;

use axum::Router;
use tokio::{select, signal, task::JoinHandle};
use tracing::{debug, error, info, warn};

#[cfg(feature = "with-db")]
use crate::db;
use crate::{
    app::{AppContext, Hooks, Initializer},
    banner::print_banner,
    bgworker,
    config::{self, Config, WorkerMode},
    controller::ListRoutes,
    environment::Environment,
    errors::Error,
    mailer::{EmailSender, MailerWorker},
    prelude::BackgroundWorker,
    task::{self, Tasks},
    Result,
};

/// Represents the application startup mode.
#[derive(Debug)]
pub enum StartMode {
    /// Run the application as a server only. when running web server only,
    /// workers job will not handle.
    ServerOnly,
    /// Run the application web server and the worker in the same process.
    ServerAndWorker,
    /// Pulling job worker and execute them
    WorkerOnly {
        /// Specifies that the worker should only handle jobs associated with one of these tags.
        /// If empty, the worker handles all jobs.
        tags: Vec<String>,
    },
}

pub struct BootResult {
    /// Application Context
    pub app_context: AppContext,
    /// Web server routes
    pub router: Option<Router>,
    /// worker processor
    pub worker: Option<Vec<String>>,
}

/// Configuration structure for serving an application.
#[derive(Debug)]
pub struct ServeParams {
    /// The port number on which the server will listen for incoming
    /// connections.
    pub port: i32,
    /// The network address to which the server will bind. It specifies the
    /// interface to listen on.
    pub binding: String,
}

/// Runs the application based on the provided `BootResult`.
///
/// This function is responsible for starting the application, including the
/// server and/or workers.
///
/// # Errors
///
/// When could not initialize the application.
pub async fn start<H: Hooks>(
    boot: BootResult,
    server_config: ServeParams,
    no_banner: bool,
) -> Result<()> {
    if !no_banner {
        print_banner(&boot, &server_config);
    }

    let BootResult {
        router,
        worker,
        app_context,
    } = boot;

    match (router, worker) {
        (Some(router), None) => {
            H::serve(router, &app_context, &server_config).await?;
        }
        (Some(router), Some(tags)) => {
            let handle = if app_context.config.workers.mode == WorkerMode::BackgroundQueue {
                Some(start_queue_worker(&app_context, tags)?)
            } else {
                None
            };

            H::serve(router, &app_context, &server_config).await?;

            if let Some(handle) = handle {
                shutdown_and_await_queue_worker(&app_context, handle).await?;
            }
        }
        (None, Some(tags)) => {
            let handle = if app_context.config.workers.mode == WorkerMode::BackgroundQueue {
                Some(start_queue_worker(&app_context, tags)?)
            } else {
                None
            };

            shutdown_signal().await;

            if let Some(handle) = handle {
                shutdown_and_await_queue_worker(&app_context, handle).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn start_queue_worker(app_context: &AppContext, tags: Vec<String>) -> Result<JoinHandle<()>> {
    debug!("note: worker is run in-process (tokio spawn)");

    if let Some(queue) = &app_context.queue_provider {
        let cloned_queue = queue.clone();
        let handle = tokio::spawn(async move {
            if let Err(err) = cloned_queue.run(tags).await {
                error!(err = err.to_string(), "error while running worker");
            }
        });
        return Ok(handle);
    }

    Err(Error::QueueProviderMissing)
}

async fn shutdown_and_await_queue_worker(
    app_context: &AppContext,
    handle: JoinHandle<()>,
) -> Result<(), Error> {
    if let Some(queue) = &app_context.queue_provider {
        queue.shutdown()?;
    }

    println!("press ctrl-c again to force quit");
    select! {
        _ = handle => {}
        () = shutdown_signal() => {}
    }
    Ok(())
}

/// Run task
///
/// # Errors
///
/// When running could not run the task.
pub async fn run_task<H: Hooks>(
    app_context: &AppContext,
    task: Option<&String>,
    vars: &task::Vars,
) -> Result<()> {
    let mut tasks = Tasks::default();
    H::register_tasks(&mut tasks);

    if let Some(task) = task {
        let task_span = tracing::span!(tracing::Level::DEBUG, "task", task,);
        let _guard = task_span.enter();
        tasks.run(app_context, task, vars).await?;
    } else {
        let list = tasks.list();
        for item in &list {
            println!("{:<30}[{}]", item.name, item.detail);
        }
    }
    Ok(())
}

/// Initializes the application context by loading configuration and
/// establishing connections.
///
/// # Errors
/// When has an error to create DB connection.
pub async fn create_context<H: Hooks>(
    environment: &Environment,
    config: Config,
) -> Result<AppContext> {
    if config.logger.pretty_backtrace {
        std::env::set_var("RUST_BACKTRACE", "1");
        warn!(
            "pretty backtraces are enabled (this is great for development but has a runtime cost \
             for production. disable with `logger.pretty_backtrace` in your config yaml)"
        );
    }
    #[cfg(feature = "with-db")]
    let db = db::connect(&config.database).await?;

    let mailer = if let Some(cfg) = config.mailer.as_ref() {
        create_mailer(cfg)?
    } else {
        None
    };

    let queue_provider = bgworker::create_queue_provider(&config).await?;
    let ctx = AppContext {
        environment: environment.clone(),
        #[cfg(feature = "with-db")]
        db,
        queue_provider,
        config,
        mailer,
        shared_store: Arc::new(crate::app::SharedStore::default()),
    };

    H::after_context(ctx).await
}

#[cfg(feature = "with-db")]
/// Creates an application based on the specified mode and environment.
///
/// # Errors
///
/// When could not create the application
pub async fn create_app<H: Hooks>(
    mode: StartMode,
    environment: &Environment,
    config: Config,
) -> Result<BootResult> {
    let app_context = create_context::<H>(environment, config).await?;
    db::converge::<H>(&app_context, &app_context.config.database).await?;

    if let (Some(queue), Some(config)) = (&app_context.queue_provider, &app_context.config.queue) {
        bgworker::converge(queue, config).await?;
    }

    run_app::<H>(&mode, app_context).await
}

#[cfg(not(feature = "with-db"))]
pub async fn create_app<H: Hooks>(
    mode: StartMode,
    environment: &Environment,
    config: Config,
) -> Result<BootResult> {
    let app_context = create_context::<H>(environment, config).await?;

    if let (Some(queue), Some(config)) = (&app_context.queue_provider, &app_context.config.queue) {
        bgworker::converge(queue, config).await?;
    }

    run_app::<H>(&mode, app_context).await
}

/// Run the application with the  given mode
/// # Errors
///
/// When could not create the application
pub async fn run_app<H: Hooks>(mode: &StartMode, app_context: AppContext) -> Result<BootResult> {
    H::before_run(&app_context).await?;
    let initializers = H::initializers(&app_context).await?;

    info!(
        initializers = ?initializers.iter().map(|init| init.name()).collect::<Vec<_>>().join(","),
        "initializers loaded"
    );

    for initializer in &initializers {
        initializer.before_run(&app_context).await?;
    }

    match mode {
        StartMode::ServerOnly => {
            let router = setup_routes::<H>(&app_context, &initializers).await?;
            Ok(BootResult {
                app_context,
                router: Some(router),
                worker: None,
            })
        }
        StartMode::ServerAndWorker => {
            register_workers::<H>(&app_context).await?;
            let router = setup_routes::<H>(&app_context, &initializers).await?;
            Ok(BootResult {
                app_context,
                router: Some(router),
                worker: Some(vec![]),
            })
        }
        StartMode::WorkerOnly { tags } => {
            register_workers::<H>(&app_context).await?;
            Ok(BootResult {
                app_context,
                router: None,
                worker: Some(tags.clone()),
            })
        }
    }
}

/// Sets up the application's routes based on the provided initializers and hooks.
async fn setup_routes<H: Hooks>(
    app_context: &AppContext,
    initializers: &[Box<dyn Initializer>],
) -> Result<Router> {
    let app = H::before_routes(app_context).await?;
    let app = H::routes(app_context).to_router::<H>(app_context.clone(), app)?;
    let mut router = H::after_routes(app, app_context).await?;

    for initializer in initializers {
        router = initializer.after_routes(router, app_context).await?;
    }

    Ok(router)
}

async fn register_workers<H: Hooks>(app_context: &AppContext) -> Result<()> {
    if app_context.config.workers.mode == WorkerMode::BackgroundQueue {
        if let Some(queue) = &app_context.queue_provider {
            queue.register(MailerWorker::build(app_context)).await?;
            H::connect_workers(app_context, queue).await?;
        } else {
            return Err(Error::QueueProviderMissing);
        }

        debug!("done registering workers and queues");
    }
    Ok(())
}

#[must_use]
pub fn list_endpoints<H: Hooks>(ctx: &AppContext) -> Vec<ListRoutes> {
    H::routes(ctx).collect()
}

/// Waits for a shutdown signal, either via Ctrl+C or termination signal.
///
/// # Panics
///
/// This function will panic if it fails to install the signal handlers for
/// Ctrl+C or the terminate signal on Unix-based systems.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

pub struct MiddlewareInfo {
    pub id: String,
    pub enabled: bool,
    pub detail: String,
}

#[must_use]
pub fn list_middlewares<H: Hooks>(ctx: &AppContext) -> Vec<MiddlewareInfo> {
    H::middlewares(ctx)
        .iter()
        .map(|m| MiddlewareInfo {
            id: m.name().to_string(),
            enabled: m.is_enabled(),
            detail: m.config().unwrap_or_default().to_string(),
        })
        .collect::<Vec<_>>()
}

/// Initializes an [`EmailSender`] based on the mailer configuration settings
/// ([`config::Mailer`]).
fn create_mailer(config: &config::Mailer) -> Result<Option<EmailSender>> {
    if config.stub {
        return Ok(Some(EmailSender::stub()));
    }
    if let Some(smtp) = config.smtp.as_ref() {
        if smtp.enable {
            return Ok(Some(EmailSender::smtp(smtp)?));
        }
    }
    Ok(None)
}
