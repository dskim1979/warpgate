//! MOAVI: drives the web-SSH SFTP client against a real OpenSSH server.
//!
//! Runs only when `MOAVI_SFTP_HOST` is set (CI starts an OpenSSH container):
//! `MOAVI_SFTP_HOST`, `MOAVI_SFTP_PORT`, `MOAVI_SFTP_USER`, `MOAVI_SFTP_PASSWORD`.

use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use russh::ChannelMsg;
use russh::client::AuthResult;
use russh::keys::PublicKeyOrCertificate;
use tokio::sync::mpsc;
use warpgate_web_ssh::sftp::{CHUNK_SIZE, SftpClient};

struct AcceptAnyHostKey;

impl russh::client::Handler for AcceptAnyHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Opens an `sftp` subsystem channel and bridges it to an [`SftpClient`],
/// the same way the web-SSH session bridges its channel.
async fn connect() -> anyhow::Result<Option<Arc<SftpClient>>> {
    let Some(host) = env("MOAVI_SFTP_HOST") else {
        eprintln!("MOAVI_SFTP_HOST not set; skipping");
        return Ok(None);
    };
    let port: u16 = env("MOAVI_SFTP_PORT").unwrap_or_else(|| "22".into()).parse()?;
    let user = env("MOAVI_SFTP_USER").unwrap_or_else(|| "moavi".into());
    let password = env("MOAVI_SFTP_PASSWORD").unwrap_or_default();

    let config = Arc::new(russh::client::Config::default());
    let mut session = russh::client::connect(config, (host.as_str(), port), AcceptAnyHostKey).await?;
    let auth = session.authenticate_password(user, password).await?;
    anyhow::ensure!(matches!(auth, AuthResult::Success), "password authentication failed");

    let mut channel = session.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Bytes>();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<Bytes>();
    tokio::spawn(async move {
        let _session = session; // keep the connection alive
        loop {
            tokio::select! {
                out = out_rx.recv() => match out {
                    Some(data) => {
                        if channel.data(&data[..]).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                msg = channel.wait() => match msg {
                    Some(ChannelMsg::Data { data }) => {
                        if in_tx.send(Bytes::copy_from_slice(&data)).is_err() {
                            break;
                        }
                    }
                    Some(ChannelMsg::Eof | ChannelMsg::Close) | None => break,
                    Some(_) => {}
                },
            }
        }
    });

    let client = SftpClient::start(Box::new(move |data| out_tx.send(data).is_ok()), in_rx).await?;
    Ok(Some(client))
}

#[tokio::test]
async fn list_upload_download_delete_against_openssh() -> anyhow::Result<()> {
    let Some(sftp) = connect().await? else {
        return Ok(());
    };

    let home = sftp.realpath(".").await?;
    anyhow::ensure!(home.starts_with('/'), "realpath gave {home}");

    let dir = format!("{home}/moavi-sftp-test");
    let _ = sftp.rmdir(&dir).await; // leftovers from an earlier run
    sftp.mkdir(&dir).await?;

    // upload a file larger than the pipeline window, in odd-sized pieces
    let payload: Bytes = (0..(CHUNK_SIZE * 20 + 4321))
        .map(|i| (i * 7 % 256) as u8)
        .collect();
    let file = format!("{dir}/파일 테스트.bin");
    let handle = sftp.open_write(&file).await?;
    let (tx, mut rx) = mpsc::channel(4);
    let feeder = {
        let payload = payload.clone();
        tokio::spawn(async move {
            for part in payload.chunks(50_000) {
                let _ = tx.send(Bytes::copy_from_slice(part)).await;
            }
        })
    };
    let written = sftp.upload(&handle, &mut rx).await?;
    feeder.await?;
    sftp.close(&handle).await?;
    anyhow::ensure!(written == payload.len() as u64, "wrote {written}");

    // it shows up in the listing with the right size
    let entries = sftp.read_dir(&dir).await?;
    let entry = entries
        .iter()
        .find(|e| e.name == "파일 테스트.bin")
        .ok_or_else(|| anyhow::anyhow!("uploaded file not listed: {entries:?}"))?;
    anyhow::ensure!(entry.attrs.size == Some(payload.len() as u64));
    anyhow::ensure!(!entry.attrs.is_dir());
    let home_entries = sftp.read_dir(&home).await?;
    anyhow::ensure!(
        home_entries
            .iter()
            .any(|e| e.name == "moavi-sftp-test" && e.attrs.is_dir())
    );

    // download returns the same bytes
    let handle = sftp.open_read(&file).await?;
    let (tx, mut rx) = mpsc::channel(4);
    let collector = tokio::spawn(async move {
        let mut all = BytesMut::new();
        while let Some(chunk) = rx.recv().await {
            all.extend_from_slice(&chunk);
        }
        all.freeze()
    });
    let read = sftp.download(&handle, &tx).await?;
    drop(tx);
    sftp.close(&handle).await?;
    let downloaded = collector.await?;
    anyhow::ensure!(read == payload.len() as u64, "read {read}");
    anyhow::ensure!(downloaded == payload, "downloaded bytes differ");

    // a non-empty folder can't be removed; an empty one can
    anyhow::ensure!(sftp.rmdir(&dir).await.is_err());
    sftp.remove(&file).await?;
    sftp.rmdir(&dir).await?;
    anyhow::ensure!(sftp.stat(&dir).await.is_err());
    Ok(())
}
