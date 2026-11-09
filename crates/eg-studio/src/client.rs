use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use prost::Message;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::framing::{encode, Decoder};
use crate::proto::{meta, studio};
use crate::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(5);

/// Waiters by request id; `None` once the link has closed.
type Pending = Arc<Mutex<Option<HashMap<u32, oneshot::Sender<studio::RequestResponse>>>>>;

/// Studio RPC client. Clones share one session; calls run one at a time, like the
/// official client.
#[derive(Clone)]
pub struct Studio {
    inner: Arc<Inner>,
}

struct Inner {
    tx: mpsc::Sender<Vec<u8>>,
    pending: Pending,
    /// The reader task owns the strong sender, so subscribers see `Closed` when the link ends.
    notifications: broadcast::WeakSender<studio::Notification>,
    /// Serializes calls and holds the next request id.
    next_id: tokio::sync::Mutex<u32>,
}

impl Studio {
    /// `tx` carries bytes to the device, `rx` bytes from it (e.g. `eg_transport::Link`).
    /// Spawns a reader task that ends when `rx` closes.
    pub fn open(tx: mpsc::Sender<Vec<u8>>, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) -> Studio {
        let pending: Pending = Arc::new(Mutex::new(Some(HashMap::new())));
        let (reader_notifications, _) = broadcast::channel(16);
        let notifications = reader_notifications.downgrade();
        let reader_pending = pending.clone();
        tokio::spawn(async move {
            let mut decoder = Decoder::default();
            while let Some(bytes) = rx.recv().await {
                for frame in decoder.push(&bytes) {
                    match studio::Response::decode(frame.as_slice()) {
                        Ok(studio::Response { r#type: Some(studio::response::Type::RequestResponse(resp)) }) => {
                            let waiter = reader_pending.lock().unwrap().as_mut().and_then(|p| p.remove(&resp.request_id));
                            match waiter {
                                Some(w) => drop(w.send(resp)),
                                None => log::warn!("studio: response to unknown request {}", resp.request_id),
                            }
                        }
                        Ok(studio::Response { r#type: Some(studio::response::Type::Notification(n)) }) => {
                            let _ = reader_notifications.send(n);
                        }
                        Ok(_) => log::warn!("studio: empty response"),
                        Err(e) => log::warn!("studio: undecodable frame: {e}"),
                    }
                }
            }
            // Dropping the waiters fails their calls with Closed; later calls fail at insert.
            reader_pending.lock().unwrap().take();
        });
        Studio { inner: Arc::new(Inner { tx, pending, notifications, next_id: tokio::sync::Mutex::new(0) }) }
    }

    /// Sends one request and returns the matching response's subsystem payload.
    /// Meta errors become `Error::UnlockRequired` / `Error::Rpc`.
    pub async fn call(&self, req: studio::request::Subsystem) -> Result<studio::request_response::Subsystem> {
        let mut next_id = self.inner.next_id.lock().await;
        let id = *next_id;
        *next_id = next_id.wrapping_add(1);

        let (waiter, response) = oneshot::channel();
        self.inner.pending.lock().unwrap().as_mut().ok_or(Error::Closed)?.insert(id, waiter);
        let frame = encode(&studio::Request { request_id: id, subsystem: Some(req) }.encode_to_vec());
        // The deadline also covers sending, so a stalled link cannot hold the call lock forever.
        let result = tokio::time::timeout(TIMEOUT, async {
            self.inner.tx.send(frame).await.map_err(|_| Error::Closed)?;
            response.await.map_err(|_| Error::Closed)
        })
        .await
        .unwrap_or(Err(Error::Timeout));
        if let Some(p) = self.inner.pending.lock().unwrap().as_mut() {
            p.remove(&id);
        }

        use studio::request_response::Subsystem;
        match result?.subsystem.ok_or(Error::UnexpectedResponse)? {
            Subsystem::Meta(m) => Err(meta_error(m)),
            other => Ok(other),
        }
    }

    /// Lock state changes and unsaved-changes status, pushed by the device.
    /// The receiver reports `Closed` once the link has ended.
    pub fn notifications(&self) -> broadcast::Receiver<studio::Notification> {
        match self.inner.notifications.upgrade() {
            Some(sender) => sender.subscribe(),
            None => broadcast::channel(1).1,
        }
    }
}

fn meta_error(m: meta::Response) -> Error {
    match m.response_type {
        Some(meta::response::ResponseType::SimpleError(code)) => match meta::ErrorConditions::try_from(code) {
            Ok(meta::ErrorConditions::UnlockRequired) => Error::UnlockRequired,
            Ok(c) => Error::Rpc(c),
            Err(_) => Error::Rpc(meta::ErrorConditions::Generic),
        },
        _ => Error::UnexpectedResponse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::core;

    fn get_lock_state() -> studio::request::Subsystem {
        studio::request::Subsystem::Core(core::Request {
            request_type: Some(core::request::RequestType::GetLockState(true)),
        })
    }

    fn respond(id: u32, subsystem: studio::request_response::Subsystem) -> Vec<u8> {
        let resp = studio::Response {
            r#type: Some(studio::response::Type::RequestResponse(studio::RequestResponse {
                request_id: id,
                subsystem: Some(subsystem),
            })),
        };
        encode(&resp.encode_to_vec())
    }

    /// Fake device answering each request via `answer`, after first pushing a notification.
    fn device(answer: fn(u32) -> Vec<u8>) -> Studio {
        let (tx, mut reqs) = mpsc::channel::<Vec<u8>>(4);
        let (out, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut decoder = Decoder::default();
            while let Some(bytes) = reqs.recv().await {
                for frame in decoder.push(&bytes) {
                    let req = studio::Request::decode(frame.as_slice()).unwrap();
                    let note = studio::Response {
                        r#type: Some(studio::response::Type::Notification(studio::Notification {
                            subsystem: Some(studio::notification::Subsystem::Core(core::Notification {
                                notification_type: Some(core::notification::NotificationType::LockStateChanged(1)),
                            })),
                        })),
                    };
                    // Split the notification across two chunks.
                    let wire = encode(&note.encode_to_vec());
                    let _ = out.send(wire[..2].to_vec());
                    let _ = out.send(wire[2..].to_vec());
                    let _ = out.send(answer(req.request_id));
                }
            }
        });
        Studio::open(tx, rx)
    }

    #[tokio::test]
    async fn returns_matching_response_and_notifications() {
        let studio = device(|id| {
            respond(
                id,
                studio::request_response::Subsystem::Core(core::Response {
                    response_type: Some(core::response::ResponseType::GetLockState(1)),
                }),
            )
        });
        let mut notes = studio.notifications();
        for _ in 0..2 {
            let resp = studio.call(get_lock_state()).await.unwrap();
            assert!(matches!(
                resp,
                studio::request_response::Subsystem::Core(core::Response {
                    response_type: Some(core::response::ResponseType::GetLockState(1))
                })
            ));
        }
        assert!(notes.recv().await.unwrap().subsystem.is_some());
    }

    #[tokio::test]
    async fn unlock_required_meta_error() {
        let studio = device(|id| {
            respond(
                id,
                studio::request_response::Subsystem::Meta(meta::Response {
                    response_type: Some(meta::response::ResponseType::SimpleError(
                        meta::ErrorConditions::UnlockRequired as i32,
                    )),
                }),
            )
        });
        assert!(matches!(studio.call(get_lock_state()).await, Err(Error::UnlockRequired)));
    }

    #[tokio::test(start_paused = true)]
    async fn ignores_other_ids_and_times_out() {
        let studio = device(|id| {
            respond(
                id + 100,
                studio::request_response::Subsystem::Core(core::Response { response_type: None }),
            )
        });
        assert!(matches!(studio.call(get_lock_state()).await, Err(Error::Timeout)));
    }

    #[tokio::test]
    async fn closed_link_fails() {
        let (tx, _reqs) = mpsc::channel(1);
        let (out, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        drop(out);
        let studio = Studio::open(tx, rx);
        let mut notes = studio.notifications();
        assert!(matches!(studio.call(get_lock_state()).await, Err(Error::Closed)));
        assert!(matches!(notes.recv().await, Err(broadcast::error::RecvError::Closed)));
        assert!(matches!(studio.notifications().recv().await, Err(broadcast::error::RecvError::Closed)));
    }
}
