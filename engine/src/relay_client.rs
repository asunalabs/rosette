//! Thin async client for the relay wire protocol. Fully pipelined (T3,
//! eng-review OV6): every request carries a `request_id` and the matching
//! `ServerFrame::Reply` is routed back to its caller through a pending-map,
//! so any number of requests can be in flight concurrently. Push messages
//! arrive independently on `push_rx` at any time, matching the SUBSCRIBE
//! model (amendment A12).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::bail;
use proto::attestation::AttestationToken;
use proto::framing::{read_frame, write_frame, ReadFrameError};
use proto::{
    ClientFrame, ClientMessage, Envelope, GroupSendKind, PowSolution, QueueId, RejectionCode,
    RequestId, ServerFrame, ServerMessage,
};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::TlsConnector;

use crate::tls::{pinned_client_config, relay_server_name};

/// Replies waiting to be claimed, keyed by request id. Shared between `call`
/// (inserts) and the reader task (removes + fulfills). Dropped senders are
/// how callers learn the connection died: the reader task drops the whole
/// map on exit, failing every in-flight `call` at once. `closed` makes that
/// final: a `call` that starts AFTER the reader exited would otherwise
/// insert a sender nobody will ever fulfill or drop and await it forever
/// (found by the T9 relay-restart test — the first send on a dead
/// connection hung instead of failing over to reconnect).
#[derive(Default)]
struct Pending {
    closed: bool,
    map: HashMap<RequestId, oneshot::Sender<ServerMessage>>,
}

type PendingReplies = Arc<Mutex<Pending>>;

/// The connection died under a request. Typed (not a bare string error) so
/// `ChatEngine` can downcast, reconnect, and retry — while every other error
/// (a relay rejection, an unexpected reply type) stays fatal to the call.
#[derive(Debug, thiserror::Error)]
#[error("relay connection closed")]
pub struct ConnectionClosed;

pub struct RelayClient {
    write_tx: mpsc::UnboundedSender<ClientFrame>,
    pending: PendingReplies,
    next_request_id: AtomicU64,
    pub push_rx: mpsc::UnboundedReceiver<(QueueId, Envelope)>,
}

impl RelayClient {
    /// Connect to a relay over TLS, pinning `relay_fingerprint` (the SHA-256 of
    /// the relay's cert, carried in the contact link). The handshake fails if
    /// the relay presents any other certificate.
    pub async fn connect(addr: &str, relay_fingerprint: [u8; 32]) -> anyhow::Result<Self> {
        let socket = TcpStream::connect(addr).await?;
        let connector = TlsConnector::from(pinned_client_config(relay_fingerprint));
        let tls = connector.connect(relay_server_name(), socket).await?;
        let (mut read_half, mut write_half) = tokio::io::split(tls);

        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<ClientFrame>();
        tokio::spawn(async move {
            while let Some(frame) = write_rx.recv().await {
                if write_frame(&mut write_half, &frame).await.is_err() {
                    break;
                }
            }
        });

        let pending: PendingReplies = Arc::new(Mutex::new(Pending::default()));
        let (push_tx, push_rx) = mpsc::unbounded_channel();
        let reader_pending = pending.clone();
        tokio::spawn(async move {
            loop {
                let frame: ServerFrame = match read_frame(&mut read_half).await {
                    Ok(frame) => frame,
                    Err(ReadFrameError::Closed) => break,
                    Err(_) => break,
                };
                match frame {
                    ServerFrame::Push { queue_id, envelope } => {
                        let _ = push_tx.send((queue_id, envelope));
                    }
                    ServerFrame::Reply {
                        request_id,
                        message,
                    } => {
                        // A missing entry means the caller gave up (dropped its
                        // future) — the reply is discarded, not misrouted.
                        if let Some(tx) = reader_pending.lock().unwrap().map.remove(&request_id) {
                            let _ = tx.send(message);
                        }
                    }
                }
            }
            // Connection gone: dropping the map drops every waiting sender,
            // which fails all in-flight `call`s with a closed-connection
            // error; `closed` fails every FUTURE `call` up front.
            let mut pending = reader_pending.lock().unwrap();
            pending.closed = true;
            pending.map.clear();
        });

        Ok(RelayClient {
            write_tx,
            pending,
            next_request_id: AtomicU64::new(0),
            push_rx,
        })
    }

    async fn call(&self, msg: ClientMessage) -> anyhow::Result<ServerMessage> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.closed {
                return Err(anyhow::Error::new(ConnectionClosed));
            }
            pending.map.insert(request_id, tx);
        }
        if self
            .write_tx
            .send(ClientFrame {
                request_id,
                message: msg,
            })
            .is_err()
        {
            self.pending.lock().unwrap().map.remove(&request_id);
            return Err(anyhow::Error::new(ConnectionClosed));
        }
        rx.await.map_err(|_| anyhow::Error::new(ConnectionClosed))
    }

    async fn create_queue_with_pow(
        &self,
        build: impl FnOnce(PowSolution) -> ClientMessage,
    ) -> anyhow::Result<(QueueId, [u8; 32])> {
        let challenge = match self.call(ClientMessage::RequestPowChallenge).await? {
            ServerMessage::PowChallenge(c) => c,
            other => bail!("expected PowChallenge, got {other:?}"),
        };
        let solution = challenge.solve();
        match self.call(build(solution)).await? {
            ServerMessage::QueueCreated { queue_id, send_key } => Ok((queue_id, send_key)),
            ServerMessage::Error(e) => bail!("queue creation rejected: {e:?}"),
            other => bail!("expected QueueCreated, got {other:?}"),
        }
    }

    /// T27: `attestation` is the single-use token the relay verifies offline
    /// when its gate is on; `None` is accepted while the gate is off (the
    /// default). The engine sources it from its token pool.
    pub async fn create_mailbox(
        &self,
        attestation: Option<AttestationToken>,
    ) -> anyhow::Result<(QueueId, [u8; 32])> {
        self.create_queue_with_pow(|solution| ClientMessage::CreateMailbox {
            solution,
            attestation,
        })
        .await
    }

    pub async fn create_group_inbox(
        &self,
        initial_epoch: u64,
        fan_out_to: Vec<QueueId>,
        attestation: Option<AttestationToken>,
    ) -> anyhow::Result<(QueueId, [u8; 32])> {
        self.create_queue_with_pow(|solution| ClientMessage::CreateGroupInbox {
            solution,
            initial_epoch,
            fan_out_to,
            attestation,
        })
        .await
    }

    pub async fn send_to_mailbox(
        &self,
        queue_id: QueueId,
        send_key: &[u8; 32],
        envelope: Envelope,
    ) -> anyhow::Result<()> {
        let auth_tag = proto::compute_tag(send_key, &queue_id, &envelope);
        match self
            .call(ClientMessage::SendToMailbox {
                queue_id,
                auth_tag,
                envelope,
            })
            .await?
        {
            ServerMessage::Ok => Ok(()),
            ServerMessage::Error(e) => bail!("mailbox send rejected: {e:?}"),
            other => bail!("expected Ok, got {other:?}"),
        }
    }

    /// Returns the relay's rejection code as data (not an error) — for the
    /// concurrent-commit conflict test, `EpochConflict` is an EXPECTED
    /// outcome for the loser, not a failure of the harness.
    pub async fn send_to_group_inbox(
        &self,
        queue_id: QueueId,
        send_key: &[u8; 32],
        kind: GroupSendKind,
        envelope: Envelope,
    ) -> anyhow::Result<Result<(), RejectionCode>> {
        let auth_tag = proto::compute_tag(send_key, &queue_id, &envelope);
        match self
            .call(ClientMessage::SendToGroupInbox {
                queue_id,
                kind,
                auth_tag,
                envelope,
            })
            .await?
        {
            ServerMessage::Ok => Ok(Ok(())),
            ServerMessage::Error(e) => Ok(Err(e)),
            other => bail!("expected Ok or Error, got {other:?}"),
        }
    }

    pub async fn subscribe(&self, queue_ids: Vec<QueueId>) -> anyhow::Result<()> {
        match self.call(ClientMessage::Subscribe { queue_ids }).await? {
            ServerMessage::Ok => Ok(()),
            other => bail!("expected Ok, got {other:?}"),
        }
    }

    pub async fn ack(&self, queue_id: QueueId, message_id: [u8; 16]) -> anyhow::Result<()> {
        match self
            .call(ClientMessage::Ack {
                queue_id,
                message_id,
            })
            .await?
        {
            ServerMessage::Ok => Ok(()),
            other => bail!("expected Ok, got {other:?}"),
        }
    }
}
