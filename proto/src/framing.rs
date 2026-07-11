//! Length-prefixed bincode framing, shared by both ends of the relay
//! connection (relay/src/net.rs and cli/'s relay client) so the wire framing
//! logic exists exactly once.

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn write_frame<W, T>(writer: &mut W, msg: &T) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: serde::Serialize,
{
    let bytes = crate::encode(msg);
    writer.write_u32_le(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ReadFrameError {
    #[error("connection closed")]
    Closed,
    #[error("frame length {0} exceeds sanity ceiling")]
    TooLarge(u32),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Decode(#[from] bincode::Error),
}

pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, ReadFrameError>
where
    R: AsyncReadExt + Unpin,
    T: serde::de::DeserializeOwned,
{
    let len = match reader.read_u32_le().await {
        Ok(len) => len,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(ReadFrameError::Closed)
        }
        Err(e) => return Err(e.into()),
    };
    if len as usize > crate::limits::MAX_MESSAGE_SIZE * 2 {
        return Err(ReadFrameError::TooLarge(len));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    Ok(crate::decode(&buf)?)
}
