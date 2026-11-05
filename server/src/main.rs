//! krunk-sync: the Krunk account and sync API (docs/sync/PLAN.md, PROTOCOL.md).
//!
//! `krunk-sync serve`        run migrations, then serve on BIND (default 0.0.0.0:8081)
//! `krunk-sync migrate`      run migrations and exit
//! `krunk-sync healthcheck`  exit 0 if GET http://127.0.0.1:<BIND port>/healthz answers 200
//!                           (the container health check; the runtime image has no curl)
//! `krunk-sync init-secrets --dir DIR [--hold]`  create missing secret files (see secrets.rs);
//!                           with --hold, stay running afterwards (the compose `secrets` service,
//!                           so no restart policy can turn a one-shot into a restart loop)
//! `krunk-sync secrets-check --dir DIR`  exit 0 if every secret file exists (its health check)

mod accounts;
mod api;
mod client_ip;
mod config;
mod crypto;
mod db;
mod http;
#[cfg(test)]
mod it;
mod jobs;
mod mail;
mod password;
mod ratelimit;
mod secrets;
mod sessions;
mod totp;
mod turnstile;

use std::net::SocketAddr;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "serve".into());
    match cmd.as_str() {
        "healthcheck" => healthcheck(),
        "init-secrets" => init_secrets(),
        "secrets-check" => match dir_arg() {
            Some(dir) if secrets::NAMES.iter().all(|n| secrets::read(&dir.join(n)).is_some()) => ExitCode::SUCCESS,
            Some(_) => ExitCode::FAILURE,
            None => ExitCode::from(2),
        },
        "serve" | "migrate" => {
            init_logging();
            let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("cannot start the runtime: {e}");
                    return ExitCode::FAILURE;
                }
            };
            match rt.block_on(run(cmd == "serve")) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!(error = %e, "fatal");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!(
                "usage: krunk-sync [serve|migrate|healthcheck|init-secrets --dir DIR [--hold]|secrets-check --dir DIR]"
            );
            ExitCode::from(2)
        }
    }
}

fn init_logging() {
    tracing_subscriber::fmt()
        .json()
        .with_current_span(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();
}

async fn run(serve: bool) -> Result<(), String> {
    let config = config::Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    db::migrate(&pool).await?;
    if !serve {
        tracing::info!("migrations up to date");
        return Ok(());
    }
    let trusted = client_ip::TrustedPeers::default();
    trusted.spawn_refresh(config.trusted_peer_host.clone());
    let passwords = password::Passwords::new(config.keys.password_pepper)?;
    let turnstile = turnstile::Turnstile::new(
        &config.turnstile_secret_key,
        &config.turnstile_hostname,
        config.turnstile_test_mode(),
    )?;
    if config.accounts_enabled() {
        if config.turnstile_test_mode() {
            tracing::warn!("Turnstile uses a Cloudflare TEST secret: there is no bot protection (staging only)");
        }
        let smtp = config.smtp.as_ref().expect("accounts_enabled implies SMTP");
        if smtp.tls == config::SmtpTls::None {
            tracing::warn!("SMTP without TLS (a local test sink only)");
        }
        mail::spawn_worker(pool.clone(), mail::Mailer::new(smtp)?, config.public_base_url.clone());
        tracing::info!("accounts enabled");
    } else {
        tracing::warn!(reason = config.accounts_blocker().unwrap_or(""), "accounts disabled");
    }
    jobs::spawn(pool.clone(), config.ip_retention_days);
    let listener =
        tokio::net::TcpListener::bind(config.bind).await.map_err(|e| format!("bind {}: {e}", config.bind))?;
    tracing::info!(bind = %config.bind, "listening");
    let app =
        http::router(http::AppState { config: std::sync::Arc::new(config), db: pool, trusted, passwords, turnstile });
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|e| e.to_string())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}

/// A dependency-free HTTP/1.0 probe of the local health endpoint.
fn healthcheck() -> ExitCode {
    use std::io::{Read, Write};
    use std::time::Duration;
    let port = config::bind_from(&|k| std::env::var(k).ok()).map(|a| a.port()).unwrap_or(8081);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let probe = || -> std::io::Result<bool> {
        let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut buf = [0u8; 64];
        let n = s.read(&mut buf)?;
        let line = String::from_utf8_lossy(&buf[..n]);
        Ok(line.starts_with("HTTP/1.0 200") || line.starts_with("HTTP/1.1 200"))
    };
    match probe() {
        Ok(true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

fn dir_arg() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == "--dir").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from)
}

fn init_secrets() -> ExitCode {
    let Some(dir) = dir_arg() else {
        eprintln!("usage: krunk-sync init-secrets --dir DIR [--hold]");
        return ExitCode::from(2);
    };
    match secrets::init(&dir) {
        Ok(actions) => {
            // Names only, never values.
            for (name, action) in actions {
                println!("{name}: {}", if action == secrets::Action::Created { "created" } else { "kept" });
            }
            if std::env::args().any(|a| a == "--hold") {
                // Compose runs this with `init: true`, which forwards SIGTERM.
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("init-secrets: {e}");
            ExitCode::FAILURE
        }
    }
}
