//! TCP + TLS framing: a thin adapter over `RelayState`. One connection = one
//! SUBSCRIBE session (amendment A12) — requests and async pushes share the
//! same socket via a single writer task so they never interleave mid-frame.
//!
//! T2 (OV2): every connection is wrapped in rustls before any frame is read,
//! so the per-queue send keys in `QueueCreated` and all ciphertext travel
//! encrypted. The relay authenticates via a pinned self-signed cert (see
//! `identity.rs`); the framing layer below is transport-agnostic and works
//! unchanged over the TLS stream.

use std::sync::Arc;

use proto::framing::{read_frame, write_frame, ReadFrameError};
use proto::{ClientMessage, ServerMessage};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};

use crate::identity::RelayIdentity;
use crate::state::RelayState;

pub async fn serve(
    addr: &str,
    state: Arc<RelayState>,
    identity: &RelayIdentity,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!(
        "relay listening on {addr} (TLS fingerprint {})",
        identity.fingerprint_hex()
    );
    serve_on(listener, state, identity).await
}

/// Split from `serve` so tests can bind an OS-assigned port (`"127.0.0.1:0"`)
/// and read back the real address before spawning the accept loop.
pub async fn serve_on(
    listener: TcpListener,
    state: Arc<RelayState>,
    identity: &RelayIdentity,
) -> anyhow::Result<()> {
    let acceptor = TlsAcceptor::from(identity.server_config());
    loop {
        let (socket, peer) = listener.accept().await?;
        let state = state.clone();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            let tls = match acceptor.accept(socket).await {
                Ok(tls) => tls,
                Err(e) => {
                    warn!(%peer, error = %e, "TLS handshake failed");
                    return;
                }
            };
            if let Err(e) = handle_connection(tls, state).await {
                warn!(%peer, error = %e, "connection ended with error");
            }
        });
    }
}

async fn handle_connection<S>(socket: S, state: Arc<RelayState>) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut read_half, write_half) = tokio::io::split(socket);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerMessage>();

    let mut write_half = write_half;
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if write_frame(&mut write_half, &msg).await.is_err() {
                break;
            }
        }
    });

    // Pushes land on this connection's own channel; forwarded straight into
    // the shared writer queue so they interleave safely with Ok/Error replies.
    let (push_tx, mut push_rx) = mpsc::unbounded_channel();
    let forward_tx = out_tx.clone();
    let forward_task = tokio::spawn(async move {
        while let Some((queue_id, _message_id, envelope)) = push_rx.recv().await {
            if forward_tx
                .send(ServerMessage::Push { queue_id, envelope })
                .is_err()
            {
                break;
            }
        }
    });

    loop {
        let msg: ClientMessage = match read_frame(&mut read_half).await {
            Ok(msg) => msg,
            Err(ReadFrameError::Closed) => break,
            Err(e) => return Err(e.into()),
        };
        let reply = state.handle(msg, Some(push_tx.clone()));
        if out_tx.send(reply).is_err() {
            break;
        }
    }

    drop(out_tx);
    let _ = writer_task.await;
    forward_task.abort();
    Ok(())
}
