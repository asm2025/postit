use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use tokio_util::sync::CancellationToken;

/// Installs the `ring` provider (the workspace never links aws-lc) and loads the PEM pair.
///
/// # Errors
///
/// Fails when the certificate or key cannot be read or parsed.
pub async fn tls_config(cert: &Path, key: &Path) -> anyhow::Result<RustlsConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    RustlsConfig::from_pem_file(cert, key)
        .await
        .with_context(|| {
            format!(
                "loading TLS cert {} / key {}",
                cert.display(),
                key.display()
            )
        })
}

/// Serves `router` on `listener` until `shutdown`, then drains for up to `grace`.
/// Served with connect info: the client-IP layer needs the peer `SocketAddr`.
///
/// # Errors
///
/// Fails when the listener cannot be handed to the server or serving fails.
pub async fn serve(
    listener: TcpListener,
    router: Router,
    tls: Option<RustlsConfig>,
    shutdown: CancellationToken,
    grace: Duration,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let handle = Handle::new();
    let signal = handle.clone();
    tokio::spawn(async move {
        shutdown.cancelled().await;
        signal.graceful_shutdown(Some(grace));
    });
    let app = router.into_make_service_with_connect_info::<SocketAddr>();
    match tls {
        Some(config) => {
            axum_server::from_tcp_rustls(listener, config)?
                .handle(handle)
                .serve(app)
                .await?;
        }
        None => {
            axum_server::from_tcp(listener)?
                .handle(handle)
                .serve(app)
                .await?;
        }
    }
    Ok(())
}
