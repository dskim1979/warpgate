//! Minimal SFTP v3 client (draft-ietf-secsh-filexfer-02) over a byte transport.
//!
//! MOAVI build: the web-SSH file panel opens an `sftp` subsystem channel on the
//! session's existing target connection and speaks SFTP over it with this
//! client, so no extra credentials are involved and the target's access rules
//! still apply. Only the requests the panel needs are implemented.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::{Mutex, mpsc, oneshot};

const SSH_FXP_INIT: u8 = 1;
const SSH_FXP_VERSION: u8 = 2;
const SSH_FXP_OPEN: u8 = 3;
const SSH_FXP_CLOSE: u8 = 4;
const SSH_FXP_READ: u8 = 5;
const SSH_FXP_WRITE: u8 = 6;
const SSH_FXP_OPENDIR: u8 = 11;
const SSH_FXP_READDIR: u8 = 12;
const SSH_FXP_REMOVE: u8 = 13;
const SSH_FXP_MKDIR: u8 = 14;
const SSH_FXP_RMDIR: u8 = 15;
const SSH_FXP_REALPATH: u8 = 16;
const SSH_FXP_STAT: u8 = 17;

const SSH_FXP_STATUS: u8 = 101;
const SSH_FXP_HANDLE: u8 = 102;
const SSH_FXP_DATA: u8 = 103;
const SSH_FXP_NAME: u8 = 104;
const SSH_FXP_ATTRS: u8 = 105;

pub const SSH_FX_OK: u32 = 0;
pub const SSH_FX_EOF: u32 = 1;
pub const SSH_FX_NO_SUCH_FILE: u32 = 2;
pub const SSH_FX_PERMISSION_DENIED: u32 = 3;

const SSH_FXF_READ: u32 = 0x01;
const SSH_FXF_WRITE: u32 = 0x02;
const SSH_FXF_CREAT: u32 = 0x08;
const SSH_FXF_TRUNC: u32 = 0x10;

const ATTR_SIZE: u32 = 0x0000_0001;
const ATTR_UIDGID: u32 = 0x0000_0002;
const ATTR_PERMISSIONS: u32 = 0x0000_0004;
const ATTR_ACMODTIME: u32 = 0x0000_0008;
const ATTR_EXTENDED: u32 = 0x8000_0000;

/// Bytes per READ/WRITE request. Every SFTP server accepts 32 KiB.
pub const CHUNK_SIZE: usize = 32 * 1024;
/// Requests kept in flight during a transfer (bounds memory to about 512 KiB).
pub const PIPELINE_DEPTH: usize = 16;
/// Largest packet accepted from the server.
const MAX_PACKET: usize = 256 * 1024 + 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const INIT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum SftpError {
    #[error("{message} (SFTP status {code})")]
    Status { code: u32, message: String },
    #[error("SFTP channel closed")]
    Closed,
    #[error("SFTP protocol error: {0}")]
    Protocol(String),
    #[error("SFTP request timed out")]
    Timeout,
}

impl SftpError {
    pub const fn status_code(&self) -> Option<u32> {
        match self {
            Self::Status { code, .. } => Some(*code),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileAttrs {
    pub size: Option<u64>,
    pub permissions: Option<u32>,
    pub mtime: Option<u32>,
}

impl FileAttrs {
    fn file_type(&self) -> Option<u32> {
        self.permissions.map(|p| p & 0o170_000)
    }

    pub fn is_dir(&self) -> bool {
        self.file_type() == Some(0o040_000)
    }

    pub fn is_symlink(&self) -> bool {
        self.file_type() == Some(0o120_000)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub attrs: FileAttrs,
}

#[derive(Debug)]
enum Reply {
    Status { code: u32, message: String },
    Handle(Bytes),
    Data(Bytes),
    Name(Vec<DirEntry>),
    Attrs(FileAttrs),
}

type Pending = Arc<Mutex<HashMap<u32, oneshot::Sender<Reply>>>>;
type Output = Box<dyn Fn(Bytes) -> bool + Send + Sync>;

/// A reply that has been requested but not awaited yet (for pipelining).
pub struct PendingReply(oneshot::Receiver<Reply>);

impl PendingReply {
    async fn wait(self) -> Result<Reply, SftpError> {
        match tokio::time::timeout(REQUEST_TIMEOUT, self.0).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(SftpError::Closed),
            Err(_) => Err(SftpError::Timeout),
        }
    }
}

pub struct SftpClient {
    out: Output,
    pending: Pending,
    next_id: AtomicU32,
    closed: Arc<AtomicBool>,
}

// ---- wire helpers ---------------------------------------------------------

fn put_string(buf: &mut BytesMut, s: &[u8]) {
    buf.put_u32(s.len() as u32);
    buf.put_slice(s);
}

fn get_u32(b: &mut Bytes) -> Result<u32, SftpError> {
    if b.remaining() < 4 {
        return Err(SftpError::Protocol("short packet".into()));
    }
    Ok(b.get_u32())
}

fn get_u64(b: &mut Bytes) -> Result<u64, SftpError> {
    if b.remaining() < 8 {
        return Err(SftpError::Protocol("short packet".into()));
    }
    Ok(b.get_u64())
}

fn get_bytes(b: &mut Bytes) -> Result<Bytes, SftpError> {
    let len = get_u32(b)? as usize;
    if b.remaining() < len {
        return Err(SftpError::Protocol("short string".into()));
    }
    Ok(b.split_to(len))
}

fn get_string(b: &mut Bytes) -> Result<String, SftpError> {
    Ok(String::from_utf8_lossy(&get_bytes(b)?).into_owned())
}

fn get_attrs(b: &mut Bytes) -> Result<FileAttrs, SftpError> {
    let flags = get_u32(b)?;
    let mut attrs = FileAttrs::default();
    if flags & ATTR_SIZE != 0 {
        attrs.size = Some(get_u64(b)?);
    }
    if flags & ATTR_UIDGID != 0 {
        get_u32(b)?;
        get_u32(b)?;
    }
    if flags & ATTR_PERMISSIONS != 0 {
        attrs.permissions = Some(get_u32(b)?);
    }
    if flags & ATTR_ACMODTIME != 0 {
        get_u32(b)?; // atime
        attrs.mtime = Some(get_u32(b)?);
    }
    if flags & ATTR_EXTENDED != 0 {
        let count = get_u32(b)?;
        for _ in 0..count {
            get_bytes(b)?;
            get_bytes(b)?;
        }
    }
    Ok(attrs)
}

/// Frames a request: length, type, id, body.
fn frame(ty: u8, id: Option<u32>, body: &[u8]) -> Bytes {
    let id_len = if id.is_some() { 4 } else { 0 };
    let mut buf = BytesMut::with_capacity(4 + 1 + id_len + body.len());
    buf.put_u32((1 + id_len + body.len()) as u32);
    buf.put_u8(ty);
    if let Some(id) = id {
        buf.put_u32(id);
    }
    buf.put_slice(body);
    buf.freeze()
}

/// Splits complete packets (type + payload, without the length) off `buf`.
fn take_packet(buf: &mut BytesMut) -> Result<Option<(u8, Bytes)>, SftpError> {
    if buf.len() < 4 {
        return Ok(None);
    }
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(buf.get(..4).unwrap_or(&[0, 0, 0, 0]));
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len == 0 || len > MAX_PACKET {
        return Err(SftpError::Protocol(format!("bad packet length {len}")));
    }
    if buf.len() < 4 + len {
        return Ok(None);
    }
    buf.advance(4);
    let mut packet = buf.split_to(len).freeze();
    let ty = packet.get_u8();
    Ok(Some((ty, packet)))
}

fn parse_reply(ty: u8, mut body: Bytes) -> Result<(u32, Reply), SftpError> {
    let id = get_u32(&mut body)?;
    let reply = match ty {
        SSH_FXP_STATUS => {
            let code = get_u32(&mut body)?;
            let message = if body.has_remaining() {
                get_string(&mut body).unwrap_or_default()
            } else {
                String::new()
            };
            Reply::Status { code, message }
        }
        SSH_FXP_HANDLE => Reply::Handle(get_bytes(&mut body)?),
        SSH_FXP_DATA => Reply::Data(get_bytes(&mut body)?),
        SSH_FXP_NAME => {
            let count = get_u32(&mut body)?;
            let mut entries = Vec::with_capacity(count.min(1024) as usize);
            for _ in 0..count {
                let name = get_string(&mut body)?;
                let _longname = get_bytes(&mut body)?;
                let attrs = get_attrs(&mut body)?;
                entries.push(DirEntry { name, attrs });
            }
            Reply::Name(entries)
        }
        SSH_FXP_ATTRS => Reply::Attrs(get_attrs(&mut body)?),
        other => return Err(SftpError::Protocol(format!("unexpected packet type {other}"))),
    };
    Ok((id, reply))
}

fn status_error(code: u32, message: String) -> SftpError {
    let message = if message.is_empty() {
        match code {
            SSH_FX_EOF => "end of file".to_owned(),
            SSH_FX_NO_SUCH_FILE => "no such file".to_owned(),
            SSH_FX_PERMISSION_DENIED => "permission denied".to_owned(),
            _ => "failure".to_owned(),
        }
    } else {
        message
    };
    SftpError::Status { code, message }
}

fn unexpected(reply: &Reply) -> SftpError {
    SftpError::Protocol(format!("unexpected reply {reply:?}"))
}

impl SftpClient {
    /// Sends INIT over `out`, waits for VERSION on `incoming`, then keeps
    /// reading replies in a background task.
    pub async fn start(
        out: Output,
        mut incoming: mpsc::UnboundedReceiver<Bytes>,
    ) -> Result<Arc<Self>, SftpError> {
        let mut init = BytesMut::new();
        init.put_u32(3);
        if !out(frame(SSH_FXP_INIT, None, &init)) {
            return Err(SftpError::Closed);
        }

        let mut buf = BytesMut::new();
        let version = tokio::time::timeout(INIT_TIMEOUT, async {
            loop {
                if let Some((ty, mut body)) = take_packet(&mut buf)? {
                    if ty != SSH_FXP_VERSION {
                        return Err(SftpError::Protocol(format!("expected VERSION, got {ty}")));
                    }
                    return get_u32(&mut body);
                }
                match incoming.recv().await {
                    Some(data) => buf.extend_from_slice(&data),
                    None => return Err(SftpError::Closed),
                }
            }
        })
        .await
        .map_err(|_| SftpError::Timeout)??;
        if version < 3 {
            return Err(SftpError::Protocol(format!("server speaks SFTP v{version}")));
        }

        let client = Arc::new(Self {
            out,
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU32::new(1),
            closed: Arc::new(AtomicBool::new(false)),
        });

        let pending = client.pending.clone();
        let closed = client.closed.clone();
        tokio::spawn(async move {
            'outer: loop {
                loop {
                    match take_packet(&mut buf) {
                        Ok(Some((ty, body))) => match parse_reply(ty, body) {
                            Ok((id, reply)) => {
                                if let Some(tx) = pending.lock().await.remove(&id) {
                                    let _ = tx.send(reply);
                                }
                            }
                            Err(e) => {
                                tracing::warn!(?e, "Dropping malformed SFTP reply");
                            }
                        },
                        Ok(None) => break,
                        Err(e) => {
                            tracing::warn!(?e, "SFTP stream corrupted");
                            break 'outer;
                        }
                    }
                }
                match incoming.recv().await {
                    Some(data) => buf.extend_from_slice(&data),
                    None => break,
                }
            }
            closed.store(true, Ordering::SeqCst);
            // dropping the senders fails every waiting request with `Closed`
            pending.lock().await.clear();
        });

        Ok(client)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn send(&self, ty: u8, body: &[u8]) -> Result<PendingReply, SftpError> {
        if self.is_closed() {
            return Err(SftpError::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if !(self.out)(frame(ty, Some(id), body)) {
            self.pending.lock().await.remove(&id);
            return Err(SftpError::Closed);
        }
        Ok(PendingReply(rx))
    }

    async fn request(&self, ty: u8, body: &[u8]) -> Result<Reply, SftpError> {
        self.send(ty, body).await?.wait().await
    }

    async fn expect_ok(&self, ty: u8, body: &[u8]) -> Result<(), SftpError> {
        match self.request(ty, body).await? {
            Reply::Status { code: SSH_FX_OK, .. } => Ok(()),
            Reply::Status { code, message } => Err(status_error(code, message)),
            other => Err(unexpected(&other)),
        }
    }

    async fn expect_handle(&self, ty: u8, body: &[u8]) -> Result<Bytes, SftpError> {
        match self.request(ty, body).await? {
            Reply::Handle(handle) => Ok(handle),
            Reply::Status { code, message } => Err(status_error(code, message)),
            other => Err(unexpected(&other)),
        }
    }

    fn path_body(path: &str) -> BytesMut {
        let mut body = BytesMut::new();
        put_string(&mut body, path.as_bytes());
        body
    }

    pub async fn realpath(&self, path: &str) -> Result<String, SftpError> {
        match self.request(SSH_FXP_REALPATH, &Self::path_body(path)).await? {
            Reply::Name(entries) => entries
                .into_iter()
                .next()
                .map(|e| e.name)
                .ok_or_else(|| SftpError::Protocol("empty REALPATH reply".into())),
            Reply::Status { code, message } => Err(status_error(code, message)),
            other => Err(unexpected(&other)),
        }
    }

    pub async fn stat(&self, path: &str) -> Result<FileAttrs, SftpError> {
        match self.request(SSH_FXP_STAT, &Self::path_body(path)).await? {
            Reply::Attrs(attrs) => Ok(attrs),
            Reply::Status { code, message } => Err(status_error(code, message)),
            other => Err(unexpected(&other)),
        }
    }

    /// Lists a directory, without `.` and `..`.
    pub async fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, SftpError> {
        let handle = self
            .expect_handle(SSH_FXP_OPENDIR, &Self::path_body(path))
            .await?;
        let mut handle_body = BytesMut::new();
        put_string(&mut handle_body, &handle);
        let mut entries = Vec::new();
        let result = loop {
            match self.request(SSH_FXP_READDIR, &handle_body).await {
                Ok(Reply::Name(batch)) => entries.extend(
                    batch
                        .into_iter()
                        .filter(|e| e.name != "." && e.name != ".."),
                ),
                Ok(Reply::Status {
                    code: SSH_FX_EOF, ..
                }) => break Ok(()),
                Ok(Reply::Status { code, message }) => break Err(status_error(code, message)),
                Ok(other) => break Err(unexpected(&other)),
                Err(e) => break Err(e),
            }
        };
        let _ = self.expect_ok(SSH_FXP_CLOSE, &handle_body).await;
        result.map(|()| entries)
    }

    pub async fn mkdir(&self, path: &str) -> Result<(), SftpError> {
        let mut body = Self::path_body(path);
        body.put_u32(0); // no attributes
        self.expect_ok(SSH_FXP_MKDIR, &body).await
    }

    pub async fn rmdir(&self, path: &str) -> Result<(), SftpError> {
        self.expect_ok(SSH_FXP_RMDIR, &Self::path_body(path)).await
    }

    pub async fn remove(&self, path: &str) -> Result<(), SftpError> {
        self.expect_ok(SSH_FXP_REMOVE, &Self::path_body(path)).await
    }

    async fn open(&self, path: &str, pflags: u32) -> Result<Bytes, SftpError> {
        let mut body = Self::path_body(path);
        body.put_u32(pflags);
        body.put_u32(0); // no attributes
        self.expect_handle(SSH_FXP_OPEN, &body).await
    }

    pub async fn open_read(&self, path: &str) -> Result<Bytes, SftpError> {
        self.open(path, SSH_FXF_READ).await
    }

    pub async fn open_write(&self, path: &str) -> Result<Bytes, SftpError> {
        self.open(path, SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC)
            .await
    }

    pub async fn close(&self, handle: &[u8]) -> Result<(), SftpError> {
        let mut body = BytesMut::new();
        put_string(&mut body, handle);
        self.expect_ok(SSH_FXP_CLOSE, &body).await
    }

    async fn send_read(
        &self,
        handle: &[u8],
        offset: u64,
        len: u32,
    ) -> Result<PendingReply, SftpError> {
        let mut body = BytesMut::new();
        put_string(&mut body, handle);
        body.put_u64(offset);
        body.put_u32(len);
        self.send(SSH_FXP_READ, &body).await
    }

    async fn send_write(
        &self,
        handle: &[u8],
        offset: u64,
        data: &[u8],
    ) -> Result<PendingReply, SftpError> {
        let mut body = BytesMut::with_capacity(4 + handle.len() + 8 + 4 + data.len());
        put_string(&mut body, handle);
        body.put_u64(offset);
        put_string(&mut body, data);
        self.send(SSH_FXP_WRITE, &body).await
    }

    /// Streams a whole file into `sink`, keeping at most [`PIPELINE_DEPTH`]
    /// reads in flight. Returns the number of bytes read; stops early (without
    /// error) when `sink` is closed.
    pub async fn download(
        &self,
        handle: &[u8],
        sink: &mpsc::Sender<Bytes>,
    ) -> Result<u64, SftpError> {
        let mut offset: u64 = 0; // next byte to request
        let mut delivered: u64 = 0; // next byte to hand to the sink
        let mut in_flight: VecDeque<(u64, PendingReply)> = VecDeque::new();
        let mut eof = false;
        loop {
            while !eof && in_flight.len() < PIPELINE_DEPTH {
                let pending = self.send_read(handle, offset, CHUNK_SIZE as u32).await?;
                in_flight.push_back((offset, pending));
                offset += CHUNK_SIZE as u64;
            }
            let Some((at, pending)) = in_flight.pop_front() else {
                return Ok(delivered);
            };
            match pending.wait().await? {
                Reply::Data(data) => {
                    if at != delivered {
                        // a previous short read restarted the pipeline; stale reply
                        continue;
                    }
                    let n = data.len() as u64;
                    if sink.send(data).await.is_err() {
                        return Ok(delivered);
                    }
                    delivered += n;
                    if n < CHUNK_SIZE as u64 {
                        // short read: drop the rest and continue right after it
                        for (_, stale) in in_flight.drain(..) {
                            let _ = stale.wait().await;
                        }
                        offset = delivered;
                    }
                }
                Reply::Status {
                    code: SSH_FX_EOF, ..
                } => {
                    eof = true;
                    for (_, stale) in in_flight.drain(..) {
                        let _ = stale.wait().await;
                    }
                }
                Reply::Status { code, message } => return Err(status_error(code, message)),
                other => return Err(unexpected(&other)),
            }
        }
    }

    /// Writes chunks from `source` sequentially, keeping at most
    /// [`PIPELINE_DEPTH`] writes in flight. Returns the number of bytes written.
    pub async fn upload(
        &self,
        handle: &[u8],
        source: &mut mpsc::Receiver<Bytes>,
    ) -> Result<u64, SftpError> {
        let mut offset: u64 = 0;
        let mut in_flight: VecDeque<PendingReply> = VecDeque::new();
        while let Some(chunk) = source.recv().await {
            for part in chunk.chunks(CHUNK_SIZE) {
                if in_flight.len() >= PIPELINE_DEPTH
                    && let Some(oldest) = in_flight.pop_front()
                {
                    Self::check_write(oldest.wait().await?)?;
                }
                in_flight.push_back(self.send_write(handle, offset, part).await?);
                offset += part.len() as u64;
            }
        }
        for pending in in_flight {
            Self::check_write(pending.wait().await?)?;
        }
        Ok(offset)
    }

    fn check_write(reply: Reply) -> Result<(), SftpError> {
        match reply {
            Reply::Status { code: SSH_FX_OK, .. } => Ok(()),
            Reply::Status { code, message } => Err(status_error(code, message)),
            other => Err(unexpected(&other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_splits_packets() -> Result<(), SftpError> {
        let mut body = BytesMut::new();
        put_string(&mut body, b"/home/moavi");
        let packet = frame(SSH_FXP_REALPATH, Some(7), &body);
        let mut buf = BytesMut::from(&packet[..]);
        buf.extend_from_slice(&packet[..3]); // part of the next packet
        let (ty, mut rest) = take_packet(&mut buf)?.ok_or(SftpError::Closed)?;
        assert_eq!(ty, SSH_FXP_REALPATH);
        assert_eq!(get_u32(&mut rest)?, 7);
        assert_eq!(get_string(&mut rest)?, "/home/moavi");
        assert_eq!(take_packet(&mut buf)?, None);
        assert_eq!(buf.len(), 3);
        Ok(())
    }

    #[test]
    fn parses_name_reply_with_attrs() -> Result<(), SftpError> {
        let mut body = BytesMut::new();
        body.put_u32(9); // id
        body.put_u32(2); // count
        for (name, mode, size) in [("docs", 0o040_755u32, 4096u64), ("a.txt", 0o100_644, 12)] {
            put_string(&mut body, name.as_bytes());
            put_string(&mut body, b"longname");
            body.put_u32(ATTR_SIZE | ATTR_UIDGID | ATTR_PERMISSIONS | ATTR_ACMODTIME);
            body.put_u64(size);
            body.put_u32(1000);
            body.put_u32(1000);
            body.put_u32(mode);
            body.put_u32(1);
            body.put_u32(1_700_000_000);
        }
        let (id, reply) = parse_reply(SSH_FXP_NAME, body.freeze())?;
        assert_eq!(id, 9);
        let Reply::Name(entries) = reply else {
            return Err(SftpError::Protocol("not a NAME".into()));
        };
        assert_eq!(entries.len(), 2);
        assert!(entries.first().is_some_and(|e| e.name == "docs" && e.attrs.is_dir()));
        assert!(entries.get(1).is_some_and(|e| {
            e.name == "a.txt" && !e.attrs.is_dir() && e.attrs.size == Some(12)
        }));
        Ok(())
    }

    #[test]
    fn rejects_oversized_packets() {
        let mut buf = BytesMut::new();
        buf.put_u32(10 * 1024 * 1024);
        buf.put_u8(SSH_FXP_DATA);
        assert!(take_packet(&mut buf).is_err());
    }

    /// Plays the server side of an in-memory SFTP conversation.
    async fn fake_server(
        mut from_client: mpsc::UnboundedReceiver<Bytes>,
        to_client: mpsc::UnboundedSender<Bytes>,
        file: Bytes,
    ) {
        let mut buf = BytesMut::new();
        let mut written: Vec<u8> = Vec::new();
        while let Some(data) = from_client.recv().await {
            buf.extend_from_slice(&data);
            while let Ok(Some((ty, mut body))) = take_packet(&mut buf) {
                if ty == SSH_FXP_INIT {
                    let mut v = BytesMut::new();
                    v.put_u32(3);
                    let _ = to_client.send(frame(SSH_FXP_VERSION, None, &v));
                    continue;
                }
                let Ok(id) = get_u32(&mut body) else { return };
                let reply = match ty {
                    SSH_FXP_OPEN => {
                        let mut h = BytesMut::new();
                        put_string(&mut h, b"h1");
                        frame(SSH_FXP_HANDLE, Some(id), &h)
                    }
                    SSH_FXP_READ => {
                        let _ = get_bytes(&mut body);
                        let offset = get_u64(&mut body).unwrap_or(0) as usize;
                        let len = get_u32(&mut body).unwrap_or(0) as usize;
                        if offset >= file.len() {
                            let mut s = BytesMut::new();
                            s.put_u32(SSH_FX_EOF);
                            frame(SSH_FXP_STATUS, Some(id), &s)
                        } else {
                            let end = (offset + len).min(file.len());
                            let mut d = BytesMut::new();
                            put_string(&mut d, file.get(offset..end).unwrap_or_default());
                            frame(SSH_FXP_DATA, Some(id), &d)
                        }
                    }
                    SSH_FXP_WRITE => {
                        let _ = get_bytes(&mut body);
                        let offset = get_u64(&mut body).unwrap_or(0) as usize;
                        let data = get_bytes(&mut body).unwrap_or_default();
                        if written.len() < offset + data.len() {
                            written.resize(offset + data.len(), 0);
                        }
                        if let Some(slot) = written.get_mut(offset..offset + data.len()) {
                            slot.copy_from_slice(&data);
                        }
                        let mut s = BytesMut::new();
                        s.put_u32(SSH_FX_OK);
                        frame(SSH_FXP_STATUS, Some(id), &s)
                    }
                    _ => {
                        // CLOSE etc.: report the bytes written so far in the message
                        let mut s = BytesMut::new();
                        s.put_u32(SSH_FX_OK);
                        put_string(&mut s, written.len().to_string().as_bytes());
                        frame(SSH_FXP_STATUS, Some(id), &s)
                    }
                };
                let _ = to_client.send(reply);
            }
        }
    }

    async fn connect(file: Bytes) -> Result<Arc<SftpClient>, SftpError> {
        let (c2s_tx, c2s_rx) = mpsc::unbounded_channel::<Bytes>();
        let (s2c_tx, s2c_rx) = mpsc::unbounded_channel::<Bytes>();
        tokio::spawn(fake_server(c2s_rx, s2c_tx, file));
        SftpClient::start(Box::new(move |b| c2s_tx.send(b).is_ok()), s2c_rx).await
    }

    #[tokio::test]
    async fn pipelined_download_returns_the_whole_file() -> Result<(), SftpError> {
        let file: Bytes = (0..(CHUNK_SIZE * 5 + 123)).map(|i| (i % 251) as u8).collect();
        let client = connect(file.clone()).await?;
        let handle = client.open_read("/f").await?;
        let (tx, mut rx) = mpsc::channel(4);
        let collector = tokio::spawn(async move {
            let mut all = BytesMut::new();
            while let Some(chunk) = rx.recv().await {
                all.extend_from_slice(&chunk);
            }
            all.freeze()
        });
        let n = client.download(&handle, &tx).await?;
        drop(tx);
        let got = collector.await.map_err(|e| SftpError::Protocol(e.to_string()))?;
        assert_eq!(n, file.len() as u64);
        assert_eq!(got, file);
        Ok(())
    }

    #[tokio::test]
    async fn pipelined_upload_writes_every_byte() -> Result<(), SftpError> {
        let client = connect(Bytes::new()).await?;
        let handle = client.open_write("/up").await?;
        let (tx, mut rx) = mpsc::channel(4);
        let payload: Bytes = (0..(CHUNK_SIZE * 3 + 7)).map(|i| (i % 13) as u8).collect();
        let feeder = {
            let payload = payload.clone();
            tokio::spawn(async move {
                for part in payload.chunks(10_000) {
                    let _ = tx.send(Bytes::copy_from_slice(part)).await;
                }
            })
        };
        let n = client.upload(&handle, &mut rx).await?;
        let _ = feeder.await;
        assert_eq!(n, payload.len() as u64);
        Ok(())
    }
}
