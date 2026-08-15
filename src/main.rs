mod app;
mod config;
mod db;
mod domain;
mod mcp;
mod migration;
mod origin;
mod request_id;
mod web;

use anyhow::{Context, Result, bail};
use config::{BootstrapConfig, Config};
use domain::{Domain, Password};
use std::{env, io::Read, sync::Arc};
use tokio::{net::TcpListener, signal};
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .compact()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("frater=info,tower_http=info")),
        )
        .init();
    if let Command::Bootstrap {
        email,
        password_stdin,
    } = parse_command()?
    {
        return bootstrap_superuser(&email, password_stdin).await;
    }
    let config = Config::from_env()?;
    let db = db::connect(&config.database_url).await?;
    let mailer: Arc<dyn domain::Mailer> = match config.smtp {
        Some(settings) => Arc::new(
            domain::SmtpMailer::new(settings).context("failed to configure the SMTP mailer")?,
        ),
        None => Arc::new(domain::LogMailer),
    };
    let domain = Arc::new(
        Domain::with_options(
            db.clone(),
            config.auth,
            config.oauth,
            domain::DomainOptions {
                registration_enabled: config.registration_enabled,
                mailer,
            },
        )
        .await
        .context("failed to initialize domain")?,
    );
    let cancellation_token = CancellationToken::new();
    let application = app::router(
        domain,
        cancellation_token.child_token(),
        app::RouterConfig {
            public_url: config.public_url,
        },
    );
    let listener = TcpListener::bind(config.http_addr)
        .await
        .context("failed to bind HTTP listener")?;
    info!(%config.http_addr,"server listening");
    let result = axum::serve(listener, application)
        .with_graceful_shutdown(shutdown_signal(cancellation_token))
        .await
        .context("HTTP server failed");
    // Close the pool only after the in-flight requests end. Else a request can
    // fail because the pool went away too early.
    if let Err(error) = db.close().await {
        tracing::warn!(%error, "failed to close the database connection");
    }
    result?;
    info!("server stopped");
    Ok(())
}
async fn bootstrap_superuser(email: &str, password_stdin: bool) -> Result<()> {
    // Never accept a password through argv or environment variables where
    // process inspection can expose it. Non-interactive operators may opt into
    // reading one newline-terminated password from stdin.
    let password = if password_stdin {
        let mut value = String::new();
        std::io::stdin()
            .take(1026)
            .read_to_string(&mut value)
            .context("failed to read password from stdin")?;
        if value.len() > 1025 {
            bail!("password from stdin exceeds 1024 bytes")
        }
        if value.ends_with("\r\n") {
            value.truncate(value.len() - 2);
        } else if value.ends_with('\n') {
            value.pop();
        }
        if value.contains(['\r', '\n']) {
            bail!("--password-stdin accepts exactly one password line")
        }
        Password::new(value).map_err(anyhow::Error::msg)?
    } else {
        let password = Password::new(
            rpassword::prompt_password("Password: ").context("failed to read password")?,
        )
        .map_err(anyhow::Error::msg)?;
        let confirmation = Password::new(
            rpassword::prompt_password("Confirm password: ")
                .context("failed to confirm password")?,
        )
        .map_err(anyhow::Error::msg)?;
        if password.bytes() != confirmation.bytes() {
            bail!("passwords do not match")
        }
        drop(confirmation);
        password
    };
    let config = BootstrapConfig::from_env()?;
    let db = db::connect(&config.database_url).await?;
    domain::bootstrap_superuser(
        &db,
        &config.password_pepper,
        &config.pepper_key_id,
        email,
        &password,
    )
    .await
    .context("failed to bootstrap superuser")?;
    info!("superuser bootstrapped");
    Ok(())
}

enum Command {
    Serve,
    Bootstrap { email: String, password_stdin: bool },
}
fn parse_command() -> Result<Command> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(Command::Serve),
        [command, flag, email] if command == "bootstrap-superuser" && flag == "--email" => {
            Ok(Command::Bootstrap {
                email: email.clone(),
                password_stdin: false,
            })
        }
        [command, flag, email, stdin]
            if command == "bootstrap-superuser"
                && flag == "--email"
                && stdin == "--password-stdin" =>
        {
            Ok(Command::Bootstrap {
                email: email.clone(),
                password_stdin: true,
            })
        }
        _ => bail!("usage: frater [bootstrap-superuser --email EMAIL [--password-stdin]]"),
    }
}
async fn shutdown_signal(cancellation_token: CancellationToken) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler")
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    let signal = tokio::select! {
        () = ctrl_c => "SIGINT",
        () = terminate => "SIGTERM",
    };
    info!(%signal, "shutdown signal received, draining in-flight requests");
    cancellation_token.cancel();
}
