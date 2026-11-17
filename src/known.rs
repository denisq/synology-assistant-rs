//! Devices seen before, remembered across runs so an offline NAS stays listed
//! (greyed out, with "last seen") and can still be woken up.

use crate::protocol::NasInfo;
use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FILE: &str = "devices.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Known {
    pub info: NasInfo,
    /// Unix seconds.
    pub last_seen: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KnownDevices {
    pub devices: BTreeMap<String, Known>,
}

impl KnownDevices {
    pub fn load() -> Self {
        store::load(FILE)
    }

    pub fn save(&self) -> std::io::Result<()> {
        store::save(FILE, self)
    }

    /// Record a sighting; returns true if this is a device we didn't know.
    pub fn seen(&mut self, info: &NasInfo, now: u64) -> bool {
        let new = !self.devices.contains_key(&info.mac);
        self.devices.insert(
            info.mac.clone(),
            Known {
                info: info.clone(),
                last_seen: now,
            },
        );
        new
    }

    pub fn forget(&mut self, mac: &str) {
        self.devices.remove(mac);
    }
}
