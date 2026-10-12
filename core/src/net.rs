//! Host networking for infrastructure mode: melonDS's `Net` (a per-instance
//! Ethernet frame dispatcher) and its drivers.
//!
//! - [`Driver::Slirp`]: user-mode NAT (libslirp). No privileges, no setup;
//!   the DS gets a DHCP lease of `10.64.0.16`, gateway `10.64.0.1`,
//!   DNS `10.64.0.2`. This is what most users want.
//! - [`Driver::Pcap`]: bridge onto a real adapter with libpcap / Npcap.

use std::ffi::{CStr, CString, c_char};
use std::ptr::NonNull;

use crate::ffi;

/// A [`Net`] backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Driver {
    /// User-mode NAT through the host's sockets.
    Slirp,
    /// Raw bridging onto the adapter with this pcap device name
    /// (see [`pcap_adapters`]).
    Pcap(String),
}

/// A network adapter libpcap can bridge onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub device_name: String,
    pub friendly_name: String,
    pub description: String,
    pub mac: [u8; 6],
    pub ipv4: [u8; 4],
}

/// Errors from [`Net::set_driver`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// libpcap/Npcap missing, or the adapter could not be opened.
    PcapUnavailable,
    /// The device name contained a NUL byte.
    BadString,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::PcapUnavailable => "libpcap unavailable or adapter could not be opened",
            Self::BadString => "string contains NUL",
        })
    }
}

impl std::error::Error for NetError {}

/// Ethernet-level link to the host network, shared by any number of
/// emulated consoles (instances `0..16`). Thread-safe.
#[derive(Debug)]
pub struct Net {
    raw: NonNull<ffi::MdsNet>,
}

// SAFETY: melonDS's PacketDispatcher locks its queues; drivers are only
// touched through the same mutex-guarded paths it uses from its emu threads.
unsafe impl Send for Net {}
unsafe impl Sync for Net {}

impl Net {
    /// A link with no driver (sends are dropped).
    ///
    /// # Panics
    /// If the C++ allocation fails.
    pub fn new() -> Self {
        let raw = NonNull::new(unsafe { ffi::mdsnet_net_new() })
            .unwrap_or_else(|| panic!("mdsnet_net_new"));
        Self { raw }
    }

    /// A link already driven by `driver`.
    ///
    /// # Errors
    /// See [`Net::set_driver`].
    pub fn with_driver(driver: &Driver) -> Result<Self, NetError> {
        let net = Self::new();
        net.set_driver(driver)?;
        Ok(net)
    }

    pub(crate) const fn as_ptr(&self) -> *mut ffi::MdsNet {
        self.raw.as_ptr()
    }

    /// Install a driver, replacing any previous one.
    ///
    /// # Errors
    /// [`NetError`] if pcap is unavailable or the device cannot be opened.
    pub fn set_driver(&self, driver: &Driver) -> Result<(), NetError> {
        match driver {
            Driver::Slirp => {
                unsafe { ffi::mdsnet_net_use_slirp(self.as_ptr()) };
                Ok(())
            }
            Driver::Pcap(dev) => {
                let dev = CString::new(dev.as_str()).map_err(|_| NetError::BadString)?;
                (unsafe { ffi::mdsnet_net_use_pcap(self.as_ptr(), dev.as_ptr()) } != 0)
                    .then_some(())
                    .ok_or(NetError::PcapUnavailable)
            }
        }
    }

    /// Remove the driver.
    pub fn clear_driver(&self) {
        unsafe { ffi::mdsnet_net_clear_driver(self.as_ptr()) }
    }

    /// Whether a driver is installed.
    pub fn has_driver(&self) -> bool {
        unsafe { ffi::mdsnet_net_has_driver(self.as_ptr()) != 0 }
    }

    /// Give instance `inst` its own receive queue. [`crate::ap::WifiAp`]
    /// does this itself.
    pub fn register_instance(&self, inst: usize) {
        unsafe { ffi::mdsnet_net_register_instance(self.as_ptr(), inst as i32) }
    }

    /// Drop instance `inst`'s receive queue.
    pub fn unregister_instance(&self, inst: usize) {
        unsafe { ffi::mdsnet_net_unregister_instance(self.as_ptr(), inst as i32) }
    }

    /// Send an Ethernet frame from instance `inst`.
    pub fn send(&self, inst: usize, frame: &[u8]) -> usize {
        unsafe {
            ffi::mdsnet_net_send_packet(
                self.as_ptr(),
                inst as i32,
                frame.as_ptr(),
                frame.len() as i32,
            )
        }
        .max(0) as usize
    }

    /// Poll the driver and pop one Ethernet frame for `inst` into `buf`.
    /// Returns its length (0 = none).
    pub fn recv(&self, inst: usize, buf: &mut [u8]) -> usize {
        unsafe {
            ffi::mdsnet_net_recv_packet(
                self.as_ptr(),
                inst as i32,
                buf.as_mut_ptr(),
                buf.len() as i32,
            )
        }
        .max(0) as usize
    }
}

impl Default for Net {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        unsafe { ffi::mdsnet_net_free(self.as_ptr()) }
    }
}

fn text(raw: &[c_char]) -> String {
    let bytes: Vec<u8> = raw
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .chain([0])
        .collect();
    CStr::from_bytes_with_nul(&bytes)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Adapters available for [`Driver::Pcap`]; `None` if libpcap/Npcap is not
/// installed.
pub fn pcap_adapters() -> Option<Vec<Adapter>> {
    let n = unsafe { ffi::mdsnet_pcap_adapters(std::ptr::null_mut(), 0) };
    if n < 0 {
        return None;
    }
    let zero = ffi::MdsNetAdapter {
        device_name: [0; 128],
        friendly_name: [0; 128],
        description: [0; 128],
        mac: [0; 6],
        ipv4: [0; 4],
    };
    let mut v = vec![zero; n as usize];
    let got = unsafe { ffi::mdsnet_pcap_adapters(v.as_mut_ptr(), n) }.max(0) as usize;
    v.truncate(got.min(n as usize));
    Some(
        v.into_iter()
            .map(|a| Adapter {
                device_name: text(&a.device_name),
                friendly_name: text(&a.friendly_name),
                description: text(&a.description),
                mac: a.mac,
                ipv4: a.ipv4,
            })
            .collect(),
    )
}
