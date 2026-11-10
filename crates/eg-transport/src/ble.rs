use std::collections::HashSet;
use std::time::Duration;

use bluest::{Adapter, Characteristic, ConnectionEvent, Device};
use futures::StreamExt;
use tokio::sync::{mpsc, oneshot};
use uuid::{uuid, Uuid};

use crate::{Error, Link, Result};

const SHELL_SVC: Uuid = uuid!("c901c4e9-5770-4bf1-96b2-2dd287813e6e");
const SHELL_CHRC: Uuid = uuid!("c901c4ea-5770-4bf1-96b2-2dd287813e6e");
const STUDIO_SVC: Uuid = uuid!("00000000-0196-6107-c967-c5cfb1c2482a");
const STUDIO_CHRC: Uuid = uuid!("00000001-0196-6107-c967-c5cfb1c2482a");
/// Marshmellow writes at most 96 bytes per packet, 20 ms apart.
const CHUNK_MAX: usize = 96;
const CHUNK_GAP: Duration = Duration::from_millis(20);
const SCAN_TIME: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct DeviceInfo {
    pub name: String,
    adapter: Adapter,
    device: Device,
}

/// Lists devices exposing the shell service: already-connected ones first, then advertising ones.
pub async fn scan() -> Result<Vec<DeviceInfo>> {
    let adapter = Adapter::default().await.ok_or(Error::NoAdapter)?;
    adapter.wait_available().await?;

    let mut seen = HashSet::new();
    let mut found = Vec::new();
    // Windows matches connected devices against its cached service list, which may lack the
    // shell service, and ZMK does not advertise it. So ask each connected device directly
    // (uncached), as Web Bluetooth does.
    let connected = adapter.connected_devices().await.unwrap_or_else(|e| {
        log::warn!("scan: listing connected devices failed: {e}");
        Vec::new()
    });
    log::info!("scan: {} connected BLE devices", connected.len());
    for device in connected {
        let has_shell = tokio::time::timeout(SCAN_TIME, has_service(&device, SHELL_SVC)).await;
        let name = device.name_async().await.unwrap_or_else(|_| "Unknown".into());
        match has_shell {
            Ok(Ok(true)) => {
                log::info!("scan: {name} has the shell service");
                seen.insert(device.id());
                found.push(DeviceInfo { name, adapter: adapter.clone(), device });
            }
            Ok(Ok(false)) => log::info!("scan: {name} has no shell service"),
            Ok(Err(e)) => log::info!("scan: {name}: {e}"),
            Err(_) => log::info!("scan: {name}: service query timed out"),
        }
    }

    let mut devices = adapter
        .discover_devices(&[SHELL_SVC])
        .await?
        .take_until(Box::pin(tokio::time::sleep(SCAN_TIME)));
    while let Some(device) = devices.next().await {
        let device = match device {
            Ok(d) => d,
            Err(e) => {
                log::warn!("scan: {e}");
                continue;
            }
        };
        if !seen.insert(device.id()) {
            continue;
        }
        let name = device.name_async().await.unwrap_or_else(|_| "Unknown".into());
        log::info!("scan: found {name} {:?}, connected: {}", device.id(), device.is_connected().await);
        found.push(DeviceInfo { name, adapter: adapter.clone(), device });
    }
    Ok(found)
}

/// Uncached GATT query. Uses `uuid_async` because bluer's `discover_services_with_uuid`
/// reads UUIDs with a blocking call that panics on errors.
async fn has_service(device: &Device, uuid: Uuid) -> Result<bool> {
    for service in device.discover_services().await? {
        if service.uuid_async().await? == uuid {
            return Ok(true);
        }
    }
    Ok(false)
}

pub struct Connection {
    adapter: Adapter,
    device: Device,
    shell: Characteristic,
    studio: Option<Characteristic>,
}

pub async fn open(info: &DeviceInfo) -> Result<Connection> {
    // No-op on Windows (OS-managed). On Linux it is required when not yet connected,
    // and may fail with AlreadyConnected when the trackball is in use as a mouse.
    if !info.device.is_connected().await {
        info.adapter.connect_device(&info.device).await?;
    }
    let shell = find_chrc(&info.device, SHELL_SVC, SHELL_CHRC)
        .await?
        .ok_or(Error::NotFound("shell characteristic"))?;
    let studio = find_chrc(&info.device, STUDIO_SVC, STUDIO_CHRC).await?;
    log::debug!("studio characteristic: {}", studio.is_some());
    Ok(Connection { adapter: info.adapter.clone(), device: info.device.clone(), shell, studio })
}

async fn find_chrc(device: &Device, svc: Uuid, chrc: Uuid) -> Result<Option<Characteristic>> {
    let Some(service) = device.discover_services_with_uuid(svc).await?.into_iter().next() else {
        return Ok(None);
    };
    Ok(service.discover_characteristics_with_uuid(chrc).await?.into_iter().next())
}

impl Connection {
    /// Zephyr shell text channel. Subscribes before returning, so no response is lost.
    pub async fn shell(&self) -> Result<Link> {
        link(self.adapter.clone(), self.device.clone(), self.shell.clone()).await
    }

    /// ZMK Studio RPC channel (framed protobuf); `None` if the firmware has no Studio service.
    pub async fn studio(&self) -> Option<Result<Link>> {
        let chrc = self.studio.clone()?;
        Some(link(self.adapter.clone(), self.device.clone(), chrc).await)
    }
}

/// One task owns the characteristic and runs reading and writing concurrently, so chunked
/// writes never stall notifications. It ends, closing `rx`, when the device disconnects,
/// a read or write fails, or the caller drops `tx` or `rx`.
async fn link(adapter: Adapter, device: Device, chrc: Characteristic) -> Result<Link> {
    let chunk = chrc.max_write_len_async().await?.min(CHUNK_MAX);
    log::debug!("write chunk size: {chunk}");

    let (tx, mut out_rx) = mpsc::channel::<Vec<u8>>(16);
    let (in_tx, rx) = mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = oneshot::channel();
    tokio::spawn(async move {
        let subscribed = async {
            let events = adapter.device_connection_events(&device).await?;
            let notes = chrc.notify().await?;
            Ok::<_, bluest::Error>((events, notes))
        };
        let (mut events, mut notes) = match subscribed.await {
            Ok(s) => {
                let _ = ready_tx.send(Ok(()));
                s
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let read = async {
            loop {
                tokio::select! {
                    note = notes.next() => match note {
                        Some(Ok(data)) => if in_tx.send(data).is_err() { break },
                        Some(Err(e)) => { log::warn!("ble notify: {e}"); break }
                        None => break,
                    },
                    event = events.next() => if matches!(event, None | Some(ConnectionEvent::Disconnected)) {
                        log::debug!("ble disconnected");
                        break;
                    },
                }
            }
        };
        let write = async {
            while let Some(data) = out_rx.recv().await {
                if let Err(e) = write_chunks(&chrc, &data, chunk).await {
                    log::warn!("ble write: {e}");
                    break;
                }
            }
        };
        // Whichever side ends first ends the link.
        tokio::select! {
            _ = read => {}
            _ = write => {}
        }
    });
    ready_rx.await.expect("link task sends ready before exiting")?;

    Ok(Link { tx, rx })
}

async fn write_chunks(chrc: &Characteristic, data: &[u8], chunk: usize) -> bluest::Result<()> {
    for part in data.chunks(chunk) {
        chrc.write_without_response(part).await?;
        tokio::time::sleep(CHUNK_GAP).await;
    }
    Ok(())
}
