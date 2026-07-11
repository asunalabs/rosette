//! Thin async client for the relay wire protocol. One outstanding
//! request-reply at a time (fine for a demo/test harness — a real client
//! would pipeline); Push messages arrive independently on `push_rx` at any
//! time, matching the SUBSCRIBE model (amendment A12).

use std::sync::Arc;

use anyhow::{anyhow, bail};
use proto::framing::{read_frame, write_frame, ReadFrameError};
use proto::{ClientMessage, Envelope, GroupSendKind, PowSolution, QueueId, RejectionCode, ServerMessage};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex};

pub struct RelayClient {
    write_tx: mpsc::UnboundedSender<ClientMessage>,
    reply_slot: Arc<Mutex<Option<oneshot::Sender<ServerMessage>>>>,
    pub push_rx: mpsc::UnboundedReceiver<(QueueId, Envelope)>,
}

impl RelayClient {
    pub async fn connect(addr: &str) -> anyhow::Result<Self> {
        let socket = TcpStream::connect(addr).await?;
        let (mut read_half, mut write_half) = socket.into_split();

        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<ClientMessage>();
        tokio::spawn(async move {
            while let Some(msg) = write_rx.recv().await {
                if write_frame(&mut write_half, &msg).await.is_err() {
                    break;
                }
            }
        });

        let reply_slot: Arc<Mutex<Option<oneshot::Sender<ServerMessage>>>> = Arc::new(Mutex::new(None));
        let (push_tx, push_rx) = mpsc::unbounded_channel();
        let reader_slot = reply_slot.clone();
        tokio::spawn(async move {
            loop {
                let msg: ServerMessage = match read_frame(&mut read_half).await {
                    Ok(msg) => msg,
                    Err(ReadFrameError::Closed) => break,
                    Err(_) => break,
                };
                match msg {
                    ServerMessage::Push { queue_id, envelope } => {
                        let _ = push_tx.send((queue_id, envelope));
                    }
                    other => {
                        if let Some(tx) = reader_slot.lock().await.take() {
                            let _ = tx.send(other);
                        }
                    }
                }
            }
        });

        Ok(RelayClient {
            write_tx,
            reply_slot,
            push_rx,
        })
    }

    async fn call(&self, msg: ClientMessage) -> anyhow::Result<ServerMessage> {
        let (tx, rx) = oneshot::channel();
        {
            let mut slot = self.reply_slot.lock().await;
            if slot.is_some() {
                bail!("relay client only supports one outstanding request at a time");
            }
            *slot = Some(tx);
        }
        self.write_tx.send(msg).map_err(|_| anyhow!("relay connection closed"))?;
        rx.await.map_err(|_| anyhow!("relay connection closed before replying"))
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

    pub async fn create_mailbox(&self) -> anyhow::Result<(QueueId, [u8; 32])> {
        self.create_queue_with_pow(|solution| ClientMessage::CreateMailbox { solution }).await
    }

    pub async fn create_group_inbox(
        &self,
        initial_epoch: u64,
        fan_out_to: Vec<QueueId>,
    ) -> anyhow::Result<(QueueId, [u8; 32])> {
        self.create_queue_with_pow(|solution| ClientMessage::CreateGroupInbox {
            solution,
            initial_epoch,
            fan_out_to,
        })
        .await
    }

    pub async fn send_to_mailbox(&self, queue_id: QueueId, send_key: &[u8; 32], envelope: Envelope) -> anyhow::Result<()> {
        let auth_tag = proto::compute_tag(send_key, &queue_id, &envelope);
        match self
            .call(ClientMessage::SendToMailbox { queue_id, auth_tag, envelope })
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
            .call(ClientMessage::SendToGroupInbox { queue_id, kind, auth_tag, envelope })
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
        match self.call(ClientMessage::Ack { queue_id, message_id }).await? {
            ServerMessage::Ok => Ok(()),
            other => bail!("expected Ok, got {other:?}"),
        }
    }
}
