//! MOAVI: file panel endpoints of a web-SSH session (SFTP over the session's
//! own target connection). Mounted under `/@warpgate/api/web-ssh/sessions/:session_id/`.
//!
//! Every transfer and change is logged with the session id, so it shows up in
//! the session's log next to the shell commands.

use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use poem::http::{StatusCode, header};
use poem::web::{Data, Json, Path, Query};
use poem::{Body, Response, handler};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;
use warpgate_common::UserSessionId;
use warpgate_common_http::auth::AuthenticatedRequestContext;
use warpgate_web_clients_common::SessionAccess;

use crate::manager::WebSshClientManager;
use crate::session::WebSshSession;
use crate::sftp::{
    CHUNK_SIZE, SSH_FX_NO_SUCH_FILE, SSH_FX_PERMISSION_DENIED, SftpClient, SftpError,
};

/// Largest single upload accepted (shown in the panel).
pub const MAX_UPLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Most entries returned for one directory.
const MAX_LIST_ENTRIES: usize = 5000;

fn error(status: StatusCode, message: &str) -> poem::Error {
    poem::Error::from_string(message, status)
}

fn sftp_error(e: &SftpError) -> poem::Error {
    match e.status_code() {
        Some(SSH_FX_NO_SUCH_FILE) => error(StatusCode::NOT_FOUND, "No such file or directory"),
        Some(SSH_FX_PERMISSION_DENIED) => error(StatusCode::FORBIDDEN, "Permission denied"),
        Some(_) => error(StatusCode::CONFLICT, &e.to_string()),
        None => error(StatusCode::BAD_GATEWAY, &e.to_string()),
    }
}

async fn session_for(
    session_id: Uuid,
    ctx: &AuthenticatedRequestContext,
    manager: &WebSshClientManager,
) -> poem::Result<Arc<WebSshSession>> {
    match manager
        .access(UserSessionId(session_id), ctx.auth.user_id())
        .await
    {
        SessionAccess::Granted(session) => Ok(session),
        SessionAccess::NotFound | SessionAccess::Forbidden => {
            Err(error(StatusCode::NOT_FOUND, "Session not found"))
        }
    }
}

async fn sftp_for(
    session_id: Uuid,
    ctx: &AuthenticatedRequestContext,
    manager: &WebSshClientManager,
) -> poem::Result<(Arc<WebSshSession>, Arc<SftpClient>)> {
    let session = session_for(session_id, ctx, manager).await?;
    if !session.file_transfer_allowed() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "File transfer is disabled for this target",
        ));
    }
    let sftp = session.sftp().await.map_err(|e| {
        warn!(session=%session.id(), ?e, "Could not open SFTP");
        error(StatusCode::BAD_GATEWAY, "SFTP is not available on this server")
    })?;
    Ok((session, sftp))
}

/// Joins a directory and a single file name; rejects names that would escape it.
pub fn join_name(dir: &str, name: &str) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0')
    {
        return None;
    }
    Some(if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    })
}

fn require_path(path: Option<&str>) -> poem::Result<&str> {
    match path {
        Some(p) if !p.is_empty() && !p.contains('\0') => Ok(p),
        _ => Err(error(StatusCode::BAD_REQUEST, "Missing path")),
    }
}

/// RFC 5987 encoding for `Content-Disposition: filename*=UTF-8''...`.
pub fn encode_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Serialize)]
pub struct SessionInfoResponse {
    target_name: String,
    username: String,
    connected_at: u64,
    recording: bool,
    file_transfer: bool,
    max_upload_bytes: u64,
}

#[handler]
pub async fn info_handler(
    Path(session_id): Path<Uuid>,
    ctx: Data<&AuthenticatedRequestContext>,
    manager: Data<&Arc<WebSshClientManager>>,
) -> poem::Result<Json<SessionInfoResponse>> {
    let session = session_for(session_id, &ctx, &manager).await?;
    Ok(Json(SessionInfoResponse {
        target_name: session.target_name().to_owned(),
        username: session.username().to_owned(),
        connected_at: session.connected_at(),
        recording: session.recording_enabled().await,
        file_transfer: session.file_transfer_allowed(),
        max_upload_bytes: MAX_UPLOAD_BYTES,
    }))
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: Option<String>,
}

#[derive(Serialize)]
pub struct ListEntry {
    name: String,
    is_dir: bool,
    is_link: bool,
    size: Option<u64>,
    mtime: Option<u32>,
}

#[derive(Serialize)]
pub struct ListResponse {
    path: String,
    entries: Vec<ListEntry>,
    truncated: bool,
}

#[handler]
pub async fn list_handler(
    Path(session_id): Path<Uuid>,
    Query(query): Query<PathQuery>,
    ctx: Data<&AuthenticatedRequestContext>,
    manager: Data<&Arc<WebSshClientManager>>,
) -> poem::Result<Json<ListResponse>> {
    let (_, sftp) = sftp_for(session_id, &ctx, &manager).await?;
    let requested = query.path.filter(|p| !p.is_empty()).unwrap_or_else(|| ".".into());
    let path = sftp.realpath(&requested).await.map_err(|e| sftp_error(&e))?;
    let mut raw = sftp.read_dir(&path).await.map_err(|e| sftp_error(&e))?;
    let truncated = raw.len() > MAX_LIST_ENTRIES;
    raw.truncate(MAX_LIST_ENTRIES);

    let mut entries = Vec::with_capacity(raw.len());
    for entry in raw {
        let mut is_dir = entry.attrs.is_dir();
        let is_link = entry.attrs.is_symlink();
        if is_link
            && let Some(full) = join_name(&path, &entry.name)
            && let Ok(target) = sftp.stat(&full).await
        {
            is_dir = target.is_dir();
        }
        entries.push(ListEntry {
            name: entry.name,
            is_dir,
            is_link,
            size: entry.attrs.size,
            mtime: entry.attrs.mtime,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Json(ListResponse {
        path,
        entries,
        truncated,
    }))
}

#[handler]
pub async fn download_handler(
    Path(session_id): Path<Uuid>,
    Query(query): Query<PathQuery>,
    ctx: Data<&AuthenticatedRequestContext>,
    manager: Data<&Arc<WebSshClientManager>>,
) -> poem::Result<Response> {
    let (session, sftp) = sftp_for(session_id, &ctx, &manager).await?;
    let path = require_path(query.path.as_deref())?.to_owned();
    let attrs = sftp.stat(&path).await.map_err(|e| sftp_error(&e))?;
    if attrs.is_dir() {
        return Err(error(StatusCode::BAD_REQUEST, "Cannot download a folder"));
    }
    let handle = sftp.open_read(&path).await.map_err(|e| sftp_error(&e))?;
    let name = path.rsplit('/').next().unwrap_or("download").to_owned();
    let user_session = session.id();
    info!(
        session=%user_session,
        direction="download",
        path=%path,
        size=attrs.size.unwrap_or(0),
        "File transfer started"
    );

    let (tx, mut rx) = mpsc::channel::<Bytes>(4);
    tokio::spawn(async move {
        let result = sftp.download(&handle, &tx).await;
        let _ = sftp.close(&handle).await;
        match result {
            Ok(bytes) => info!(
                session=%user_session,
                direction="download",
                path=%path,
                size=bytes,
                "File transfer finished"
            ),
            Err(e) => warn!(
                session=%user_session,
                direction="download",
                path=%path,
                error=%e,
                "File transfer failed"
            ),
        }
    });
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|chunk| (Ok::<Bytes, std::io::Error>(chunk), rx))
    });

    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from_bytes_stream(stream));
    let disposition = format!("attachment; filename*=UTF-8''{}", encode_filename(&name));
    if let Ok(value) = disposition.parse() {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, value);
    }
    if let Some(size) = attrs.size
        && let Ok(value) = size.to_string().parse()
    {
        response.headers_mut().insert("x-file-size", value);
    }
    Ok(response)
}

#[derive(Deserialize)]
pub struct UploadQuery {
    dir: Option<String>,
    name: Option<String>,
}

#[derive(Serialize)]
pub struct UploadResponse {
    path: String,
    size: u64,
}

#[handler]
pub async fn upload_handler(
    Path(session_id): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    ctx: Data<&AuthenticatedRequestContext>,
    manager: Data<&Arc<WebSshClientManager>>,
    body: Body,
) -> poem::Result<Json<UploadResponse>> {
    let (session, sftp) = sftp_for(session_id, &ctx, &manager).await?;
    let dir = require_path(query.dir.as_deref())?;
    let path = query
        .name
        .as_deref()
        .and_then(|name| join_name(dir, name))
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Invalid file name"))?;

    let handle = sftp.open_write(&path).await.map_err(|e| sftp_error(&e))?;
    let user_session = session.id();
    info!(session=%user_session, direction="upload", path=%path, "File transfer started");

    let (tx, rx) = mpsc::channel::<Bytes>(4);
    let mut body = body.into_bytes_stream();
    let reader = async move {
        let mut total: u64 = 0;
        let mut pending = BytesMut::new();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|_| "upload interrupted")?;
            total += chunk.len() as u64;
            if total > MAX_UPLOAD_BYTES {
                return Err("file too large");
            }
            pending.extend_from_slice(&chunk);
            while pending.len() >= CHUNK_SIZE {
                let part = pending.split_to(CHUNK_SIZE).freeze();
                if tx.send(part).await.is_err() {
                    return Err("SFTP write failed");
                }
            }
        }
        if !pending.is_empty() && tx.send(pending.freeze()).await.is_err() {
            return Err("SFTP write failed");
        }
        Ok(total)
    };
    // owns `rx`, so a failed write drops it and unblocks the reader
    let writer = {
        let sftp = sftp.clone();
        let handle = handle.clone();
        async move {
            let mut rx = rx;
            sftp.upload(&handle, &mut rx).await
        }
    };
    let (read_result, write_result) = tokio::join!(reader, writer);
    let _ = sftp.close(&handle).await;

    let outcome = match (read_result, write_result) {
        (Ok(_), Ok(written)) => Ok(written),
        (Err(reason), _) => Err(reason.to_owned()),
        (Ok(_), Err(e)) => Err(e.to_string()),
    };
    match outcome {
        Ok(size) => {
            info!(
                session=%user_session,
                direction="upload",
                path=%path,
                size=size,
                "File transfer finished"
            );
            Ok(Json(UploadResponse { path, size }))
        }
        Err(reason) => {
            warn!(
                session=%user_session,
                direction="upload",
                path=%path,
                error=%reason,
                "File transfer failed"
            );
            // don't leave a partial file behind
            let _ = sftp.remove(&path).await;
            let status = if reason == "file too large" {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_GATEWAY
            };
            Err(error(status, &reason))
        }
    }
}

#[handler]
pub async fn mkdir_handler(
    Path(session_id): Path<Uuid>,
    Query(query): Query<UploadQuery>,
    ctx: Data<&AuthenticatedRequestContext>,
    manager: Data<&Arc<WebSshClientManager>>,
) -> poem::Result<StatusCode> {
    let (session, sftp) = sftp_for(session_id, &ctx, &manager).await?;
    let dir = require_path(query.dir.as_deref())?;
    let path = query
        .name
        .as_deref()
        .and_then(|name| join_name(dir, name))
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Invalid folder name"))?;
    sftp.mkdir(&path).await.map_err(|e| sftp_error(&e))?;
    info!(session=%session.id(), path=%path, "Folder created");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    path: Option<String>,
    dir: Option<bool>,
}

#[handler]
pub async fn delete_handler(
    Path(session_id): Path<Uuid>,
    Query(query): Query<DeleteQuery>,
    ctx: Data<&AuthenticatedRequestContext>,
    manager: Data<&Arc<WebSshClientManager>>,
) -> poem::Result<StatusCode> {
    let (session, sftp) = sftp_for(session_id, &ctx, &manager).await?;
    let path = require_path(query.path.as_deref())?;
    if path == "/" {
        return Err(error(StatusCode::BAD_REQUEST, "Refusing to delete /"));
    }
    if query.dir.unwrap_or(false) {
        sftp.rmdir(path).await.map_err(|e| sftp_error(&e))?;
    } else {
        sftp.remove(path).await.map_err(|e| sftp_error(&e))?;
    }
    info!(session=%session.id(), path=%path, "File deleted");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_name_rejects_escapes() {
        assert_eq!(join_name("/home/u", "a.txt").as_deref(), Some("/home/u/a.txt"));
        assert_eq!(join_name("/", "a").as_deref(), Some("/a"));
        assert_eq!(join_name("/home/u", ".."), None);
        assert_eq!(join_name("/home/u", "x/y"), None);
        assert_eq!(join_name("/home/u", ""), None);
    }

    #[test]
    fn filename_is_rfc5987_encoded() {
        assert_eq!(encode_filename("보고서 1.txt"), "%EB%B3%B4%EA%B3%A0%EC%84%9C%201.txt");
    }
}
