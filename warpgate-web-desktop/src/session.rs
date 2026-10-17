use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Sender;
use tracing::{info, warn};
use uuid::Uuid;
use warpgate_common::{UserSessionId, WebClipboardMode};
use warpgate_core::recordings::DesktopRecorder;
use warpgate_core::{DesktopEvent, DesktopInput, DesktopRect, Framebuffer, Rect};
use warpgate_web_clients_common::{ManagedSession, Sheddable, WebSession};

use crate::protocol::{ServerMessage, WsRect};

/// Initial output-buffer allocation. Not a hard cap: incremental frames are bounded by
/// [`MAX_BUFFERED_INCREMENTAL`] and structural messages are rare, so the buffer stays small.
const OUTPUT_BUFFER_CAPACITY: usize = 128;

/// How many incremental (droppable) framebuffer frames to keep buffered for a slow or
/// briefly-disconnected client. Small on purpose: a client that falls behind should get
/// the recent live edge, not a huge stale backlog to grind through (which is what froze
/// the browser on high-frequency updates). Structural messages are never counted or dropped.
const MAX_BUFFERED_INCREMENTAL: usize = 64;

impl Sheddable for ServerMessage {
    fn is_droppable(&self) -> bool {
        self.is_incremental()
    }
}

pub struct WebDesktopSession {
    core: WebSession<ServerMessage>,
    input_tx: Sender<DesktopInput>,
    // shared with the manager's event loop; records viewer input for audit
    recorder: Option<Arc<DesktopRecorder>>,
    /// Composited surface, kept so a viewer attaching mid-session gets a base image.
    /// Separate from the recorder's: that one must only see events it actually wrote.
    /// Holds the refinement scratch too, so encoding one region needs a single lock.
    framebuffer: Mutex<(Framebuffer, RefineScratch)>,
    // MOAVI: clipboard direction policy of the target, and text typed through the
    // viewer's type-it box (Unicode keysyms) not logged yet
    clipboard_mode: WebClipboardMode,
    is_vnc: bool,
    typed_chars: AtomicU64,
}

/// X11 Unicode keysyms (`0x0100_0000 + code point`) only come from the viewer's
/// type-it box, never from a physical key press.
const UNICODE_KEYSYM_BASE: u32 = 0x0100_0000;

/// Buffers reused across refinement encodes. A settling screen refines many regions back to
/// back, and `rgba` is a full copy of each region's pixels — a fresh allocation per region
/// would churn megabytes for a once-per-settle job.
#[derive(Default)]
struct RefineScratch {
    rgba: Vec<u8>,
    png: Vec<u8>,
}

impl WebDesktopSession {
    pub fn new(
        id: UserSessionId,
        user_id: Uuid,
        target_name: String,
        target_kind: warpgate_db_entities::Target::TargetKind,
        server_handle: Arc<tokio::sync::Mutex<warpgate_core::WarpgateServerHandle>>,
        input_tx: Sender<DesktopInput>,
        abort_tx: tokio::sync::mpsc::UnboundedSender<()>,
        recorder: Option<Arc<DesktopRecorder>>,
        clipboard_mode: WebClipboardMode,
    ) -> Self {
        let is_vnc = target_kind == warpgate_db_entities::Target::TargetKind::Vnc;
        Self {
            core: WebSession::new(
                id,
                user_id,
                target_name,
                target_kind,
                server_handle,
                abort_tx,
                OUTPUT_BUFFER_CAPACITY,
                MAX_BUFFERED_INCREMENTAL,
            ),
            input_tx,
            recorder,
            framebuffer: Mutex::new((Framebuffer::default(), RefineScratch::default())),
            clipboard_mode,
            is_vnc,
            typed_chars: AtomicU64::new(0),
        }
    }

    pub const fn clipboard_mode(&self) -> WebClipboardMode {
        self.clipboard_mode
    }

    /// MOAVI: applies the target's clipboard policy to viewer input. Pasted text and
    /// type-it text are "in" transfers; ordinary key presses always pass. RDP logs
    /// allowed clipboard transfers in its client (which also serves native clients),
    /// VNC here.
    pub fn admit_input(&self, input: &DesktopInput) -> bool {
        match input {
            DesktopInput::Clipboard(text) => {
                self.flush_typed();
                if !self.clipboard_mode.allows_in() {
                    info!(session=%self.core.id(), direction = "in", bytes = text.len(), "Clipboard transfer blocked by policy");
                    return false;
                }
                if self.is_vnc {
                    info!(session=%self.core.id(), direction = "in", bytes = text.len(), "Clipboard transfer");
                }
                true
            }
            DesktopInput::Key {
                keysym: Some(keysym),
                scancode: None,
                down,
            } if *keysym >= UNICODE_KEYSYM_BASE => {
                if !self.clipboard_mode.allows_in() {
                    return false;
                }
                if *down {
                    self.typed_chars.fetch_add(1, Ordering::Relaxed);
                }
                true
            }
            _ => {
                self.flush_typed();
                true
            }
        }
    }

    /// Logs text typed through the type-it box since the last call.
    pub fn flush_typed(&self) {
        let chars = self.typed_chars.swap(0, Ordering::Relaxed);
        if chars > 0 {
            info!(session=%self.core.id(), direction = "in", chars, "Text typed into the desktop");
        }
    }

    /// MOAVI: applies the target's clipboard policy to text the target copied.
    pub fn admit_clipboard_out(&self, text: &str) -> bool {
        if !self.clipboard_mode.allows_out() {
            info!(session=%self.core.id(), direction = "out", bytes = text.len(), "Clipboard transfer blocked by policy");
            return false;
        }
        if self.is_vnc {
            info!(session=%self.core.id(), direction = "out", bytes = text.len(), "Clipboard transfer");
        }
        true
    }

    /// Composite an event into the session's surface. Driven from the manager's event loop
    /// on the *pre-JPEG* stream, so this stays a plain blit with no decode round-trip.
    pub async fn composite(&self, event: &DesktopEvent) {
        self.framebuffer.lock().await.0.apply(event);
    }

    /// A full-canvas snapshot for a viewer that just attached. `None` before the first
    /// resize, when the backend hasn't reported a size yet.
    ///
    /// Encodes under the lock: it runs once per attach, and copying the surface out to
    /// offload it would cost more than the encode saves.
    pub async fn keyframe(&self) -> Option<ServerMessage> {
        let (width, height, data) = self.framebuffer.lock().await.0.snapshot_png()?;
        Some(ServerMessage::Keyframe {
            rect: WsRect {
                x: 0,
                y: 0,
                width,
                height,
            },
            data: data.into(),
        })
    }

    /// The current pixels of a region, as a lossless PNG event. Used to refine a region
    /// that was sent lossily and has since gone quiet; `None` once it falls outside the
    /// surface (a resize since it was marked dirty).
    pub async fn refinement(&self, rect: DesktopRect) -> Option<DesktopEvent> {
        let mut guard = self.framebuffer.lock().await;
        let (framebuffer, scratch) = &mut *guard;
        let clipped = framebuffer.region_png(
            Rect {
                x: u32::from(rect.x),
                y: u32::from(rect.y),
                width: u32::from(rect.width),
                height: u32::from(rect.height),
            },
            &mut scratch.rgba,
            &mut scratch.png,
        )?;
        Some(DesktopEvent::PngImage {
            rect: DesktopRect {
                x: u16::try_from(clipped.x).ok()?,
                y: u16::try_from(clipped.y).ok()?,
                width: u16::try_from(clipped.width).ok()?,
                height: u16::try_from(clipped.height).ok()?,
            },
            // Copied out so the scratch keeps its capacity for the next region.
            data: Bytes::copy_from_slice(&scratch.png),
        })
    }

    pub async fn send_input(&self, input: DesktopInput) {
        // Record the viewer's input for audit before forwarding (like native RDP/VNC).
        if let Some(recorder) = &self.recorder
            && let Err(error) = recorder.write_input(&input).await
        {
            warn!(%error, "Failed to record web-desktop viewer input");
        }
        // let inputs drop under backpressure
        let _ = self.input_tx.try_send(input);
    }
}

impl std::ops::Deref for WebDesktopSession {
    type Target = WebSession<ServerMessage>;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl ManagedSession for WebDesktopSession {
    fn id(&self) -> UserSessionId {
        self.core.id()
    }

    fn user_id(&self) -> Uuid {
        self.core.user_id()
    }

    fn on_removed(&self) {
        self.core.abort();
        self.core.close();
    }
}
