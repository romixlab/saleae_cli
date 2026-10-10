//! Devices the server knows about: real ones over USB and the simulated ones it always has.

use crate::error::{Error, Result};
use crate::pb;
use crate::server::Session;
use std::time::Duration;

/// `DeviceType` value of a Logic MSO. It is only in API 1.2 (the proto in Saleae's server zip), not in the
/// published 1.0 proto this crate builds against by default, so it is matched as a number.
/// TODO(CAP-4): use the enum once API 1.2 is published.
pub const DEVICE_TYPE_LOGIC_MSO: i32 = 7;

pub fn name(t: i32) -> &'static str {
    if t == DEVICE_TYPE_LOGIC_MSO {
        return "Logic MSO";
    }
    match pb::DeviceType::try_from(t).unwrap_or(pb::DeviceType::Unspecified) {
        pb::DeviceType::Logic => "Logic",
        pb::DeviceType::Logic4 => "Logic 4",
        pb::DeviceType::Logic8 => "Logic 8",
        pb::DeviceType::Logic16 => "Logic 16",
        pb::DeviceType::LogicPro8 => "Logic Pro 8",
        pb::DeviceType::LogicPro16 => "Logic Pro 16",
        _ => "unknown",
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub type_name: String,
    pub device_type: i32,
    pub simulated: bool,
}

/// Real devices, and simulated ones unless `real_only`.
pub async fn list(session: &mut Session, real_only: bool) -> Result<Vec<DeviceInfo>> {
    let devices = session
        .client
        .get_devices(pb::GetDevicesRequest {
            include_simulation_devices: !real_only,
        })
        .await?
        .into_inner()
        .devices;
    Ok(devices
        .into_iter()
        .map(|d| DeviceInfo {
            id: d.device_id,
            type_name: name(d.device_type).to_string(),
            device_type: d.device_type,
            simulated: d.is_simulation,
        })
        .collect())
}

/// Picks the device: the given id, else the first real one; lists the simulated ones when none is plugged in.
/// Returns its id and raw `DeviceType` value.
pub(crate) async fn resolve(session: &mut Session, device: Option<&str>) -> Result<(String, i32)> {
    let devices = session
        .client
        .get_devices(pb::GetDevicesRequest {
            include_simulation_devices: true,
        })
        .await?
        .into_inner()
        .devices;
    if let Some(d) = device {
        let Some(found) = devices.iter().find(|x| x.device_id.eq_ignore_ascii_case(d)) else {
            let known: Vec<_> = devices.iter().map(|x| x.device_id.as_str()).collect();
            return Err(Error::not_found(format!(
                "device {d} not found; available: {}",
                known.join(", ")
            )));
        };
        return Ok((found.device_id.clone(), found.device_type));
    }
    if let Some(real) = devices.iter().find(|d| !d.is_simulation) {
        return Ok((real.device_id.clone(), real.device_type));
    }
    let sims: Vec<_> = devices
        .iter()
        .map(|d| format!("{} ({})", d.device_id, name(d.device_type)))
        .collect();
    Err(Error::not_found(format!(
        "no real Saleae device connected (check USB and udev rules); pass a simulated device id: {}",
        sims.join(", ")
    )))
}

/// Device ids and type names of a server already running at `addr`, without starting one; `None` if none
/// answers within `timeout`. Meant for low-latency uses like shell completion.
pub async fn probe_quick(addr: &str, timeout: Duration) -> Option<Vec<(String, String)>> {
    let fut = async {
        let ep = tonic::transport::Endpoint::from_shared(format!("http://{addr}")).ok()?;
        let mut c = pb::manager_client::ManagerClient::new(ep.connect().await.ok()?);
        let devices = c
            .get_devices(pb::GetDevicesRequest {
                include_simulation_devices: true,
            })
            .await
            .ok()?
            .into_inner()
            .devices;
        Some(
            devices
                .into_iter()
                .map(|d| {
                    let sim = if d.is_simulation { " (simulated)" } else { "" };
                    (d.device_id, format!("{}{sim}", name(d.device_type)))
                })
                .collect(),
        )
    };
    tokio::time::timeout(timeout, fut).await.ok().flatten()
}
