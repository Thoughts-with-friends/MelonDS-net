//! melonDS's emulated access point (`WifiAP`): a minimal open AP called
//! `"melonAP"` on channel 6 that beacons, accepts auth/association, and
//! bridges the DS's 802.11 data frames (LLC/SNAP) to Ethernet frames on a
//! [`Net`]. Put it in range of an emulated console and the game's Nintendo
//! WFC settings can find it and get online.

use std::ffi::CStr;
use std::ptr::NonNull;
use std::sync::Arc;

use crate::ffi;
use crate::net::Net;

/// Bytes a receive buffer needs: 12-byte RX header + the largest frame the
/// AP crafts.
pub const RECV_BUFFER_SIZE: usize = 2048 + 12;

/// The AP's SSID.
pub fn name() -> &'static str {
    unsafe { CStr::from_ptr(ffi::mdsnet_ap_name()) }
        .to_str()
        .unwrap_or("melonAP")
}

/// The AP's BSSID.
pub fn mac() -> [u8; 6] {
    let mut m = [0; 6];
    unsafe { ffi::mdsnet_ap_mac(m.as_mut_ptr()) };
    m
}

/// The AP's channel.
pub fn channel() -> u8 {
    unsafe { ffi::mdsnet_ap_channel() }
}

/// One console's view of the emulated AP.
///
/// Feed it every frame the console transmits ([`WifiAp::send`]), tick it
/// every emulated millisecond ([`WifiAp::ms_timer`]), and poll
/// [`WifiAp::recv`] from the console's RX logic.
#[derive(Debug)]
pub struct WifiAp {
    raw: NonNull<ffi::MdsNetAP>,
    _net: Option<Arc<Net>>,
}

// SAFETY: a WifiAp is driven from one emulator thread at a time (&mut self);
// the Net it shares is itself thread-safe.
unsafe impl Send for WifiAp {}

impl WifiAp {
    /// An AP for console `inst` whose MAC is `mac` (the firmware's MAC; the
    /// AP only forwards Ethernet frames addressed to it or broadcast).
    /// With `net == None` the AP associates but carries no traffic.
    ///
    /// # Panics
    /// If the C++ allocation fails.
    pub fn new(net: Option<Arc<Net>>, inst: usize, mac: [u8; 6]) -> Self {
        let np = net.as_ref().map_or(std::ptr::null_mut(), |n| n.as_ptr());
        let raw = NonNull::new(unsafe { ffi::mdsnet_ap_new(np, inst as i32, mac.as_ptr()) })
            .unwrap_or_else(|| panic!("mdsnet_ap_new"));
        Self { raw, _net: net }
    }

    /// Change the client MAC.
    pub fn set_mac(&mut self, mac: [u8; 6]) {
        unsafe { ffi::mdsnet_ap_set_mac(self.raw.as_ptr(), mac.as_ptr()) }
    }

    /// Forget the client (console reset).
    pub fn reset(&mut self) {
        unsafe { ffi::mdsnet_ap_reset(self.raw.as_ptr()) }
    }

    /// Advance the AP clock by one emulated millisecond.
    pub fn ms_timer(&mut self) {
        unsafe { ffi::mdsnet_ap_ms_timer(self.raw.as_ptr()) }
    }

    /// A frame the console transmitted (12-byte TX header + 802.11 frame).
    /// Returns the length consumed, 0 if the frame was not for the AP.
    pub fn send(&mut self, frame: &[u8]) -> usize {
        unsafe { ffi::mdsnet_ap_send_packet(self.raw.as_ptr(), frame.as_ptr(), frame.len() as i32) }
            .max(0) as usize
    }

    /// The next frame from the AP (12-byte RX header + 802.11 frame), into
    /// `buf` (at least [`RECV_BUFFER_SIZE`]). Returns its length, 0 = none.
    pub fn recv(&mut self, buf: &mut [u8]) -> usize {
        unsafe { ffi::mdsnet_ap_recv_packet(self.raw.as_ptr(), buf.as_mut_ptr(), buf.len() as i32) }
            .max(0) as usize
    }
}

impl Drop for WifiAp {
    fn drop(&mut self) {
        unsafe { ffi::mdsnet_ap_free(self.raw.as_ptr()) }
    }
}
