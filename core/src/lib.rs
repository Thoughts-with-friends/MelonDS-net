//! # melonds_net
//!
//! Nintendo DS networking for emulators, taken from
//! [melonDS](https://github.com/melonDS-emu/melonDS)'s C++ network layer and
//! exposed to Rust through a small C ABI (`shim/mdsnet.h`).
//!
//! An emulator that models the DS Wi-Fi hardware (GBATEK "DS Wireless
//! Communications") only has to hand over the frames its MAC transmits and
//! ask for frames to receive; this crate provides the medium:
//!
//! | Module | melonDS piece | What it gives you |
//! |---|---|---|
//! | [`mp`] | `MPInterface`, `LocalMP`, `LAN` | Local wireless play between consoles (same process, or other machines) |
//! | [`lan`] | `LAN` | Hosting / joining / discovering LAN sessions |
//! | [`net`] | `Net`, `Net_Slirp`, `Net_PCap` | An Ethernet link to the host network (NAT or bridged) |
//! | [`ap`] | `WifiAP` | An emulated access point that bridges 802.11 to that link (WFC) |
//! | [`Station`] | `Wifi.cpp`'s TX/RX routing | All of the above behind two calls |
//!
//! ## Quick start
//!
//! ```no_run
//! use melonds_net::{Station, mp};
//!
//! melonds_net::init_logging();
//! mp::set_backend(mp::MpBackend::Local);           // or Lan + lan::host/join
//! let net = melonds_net::net::Net::with_driver(&melonds_net::net::Driver::Slirp).ok();
//! let mut sta = Station::new(0, [0x00, 0x09, 0xBF, 0x12, 0x34, 0x56], net.map(std::sync::Arc::new));
//!
//! // From the emulated Wi-Fi MAC:
//! # let (tx_frame, now_us) = (vec![0u8; 40], 0u64);
//! sta.transmit(melonds_net::FrameKind::Packet, &tx_frame, now_us);
//! let mut rx = vec![0u8; melonds_net::RX_BUFFER_SIZE];
//! if let Some((len, _ts)) = sta.receive(&mut rx) { /* deliver rx[..len] */ }
//! ```

pub mod ap;
pub mod ffi;
pub mod lan;
pub mod mp;
pub mod net;

use std::sync::Arc;

pub use mp::{MpBackend, MpHandle, MpPort, Recv};

/// Size of the TX/RX header melonDS puts in front of every 802.11 frame.
pub const FRAME_HEADER_SIZE: usize = 12;
/// A receive buffer big enough for anything [`Station::receive`] returns.
pub const RX_BUFFER_SIZE: usize = 0x1000;

unsafe extern "C" fn log_bridge(level: std::ffi::c_int, msg: *const std::ffi::c_char) {
    let level = match level {
        0 => log::Level::Debug,
        1 => log::Level::Info,
        2 => log::Level::Warn,
        _ => log::Level::Error,
    };
    if log::log_enabled!(target: "melonds_net", level) && !msg.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(msg) }.to_string_lossy();
        log::log!(target: "melonds_net", level, "{}", msg.trim_end());
    }
}

/// Route melonDS's log output to the [`log`] crate (target `melonds_net`).
pub fn init_logging() {
    unsafe { ffi::mdsnet_set_log(Some(log_bridge)) }
}

/// How the console's hardware sent a frame (GBATEK: the TX slot / the
/// multiplay sequence it belongs to).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// Ordinary frame (LOC1-3 slots, beacons).
    Packet,
    /// Multiplay host CMD (CMD slot).
    Cmd,
    /// Multiplay client reply, with the console's AID (`W_AID_LOW`).
    Reply { aid: u16 },
    /// Multiplay host ACK.
    Ack,
}

/// One console's whole network presence: its MP port plus (optionally) the
/// emulated access point, routed the way melonDS's `Wifi.cpp` does —
/// ordinary frames go to both MP peers and the AP, received frames come
/// from MP peers first, then the AP.
#[derive(Debug)]
pub struct Station {
    port: MpPort,
    ap: Option<ap::WifiAp>,
    ms_accum: u64,
    last_us: Option<u64>,
}

impl Station {
    /// Console `inst` with firmware MAC `mac`. With `net`, an access point
    /// bridged onto it is placed in range.
    pub fn new(inst: usize, mac: [u8; 6], net: Option<Arc<net::Net>>) -> Self {
        let ap = net.map(|n| ap::WifiAp::new(Some(n), inst, mac));
        Self {
            port: MpPort::begin(inst),
            ap,
            ms_accum: 0,
            last_us: None,
        }
    }

    /// The MP port, for multiplay receives ([`MpPort::recv_host_packet`],
    /// [`MpPort::recv_replies`]).
    pub const fn port(&self) -> &MpPort {
        &self.port
    }

    /// The access point, if any.
    pub const fn access_point(&mut self) -> Option<&mut ap::WifiAp> {
        self.ap.as_mut()
    }

    /// Advance the AP's clock to the console's Wi-Fi microsecond counter.
    /// Call it as often as convenient (e.g. once per frame).
    pub fn tick(&mut self, now_us: u64) {
        let last = self.last_us.replace(now_us).unwrap_or(now_us);
        self.ms_accum += now_us.saturating_sub(last);
        while self.ms_accum >= 1024 {
            self.ms_accum -= 1024;
            if let Some(ap) = &mut self.ap {
                ap.ms_timer();
            }
        }
    }

    /// Hand over a frame (12-byte TX header + 802.11 frame) the console
    /// transmitted at `timestamp` (µs). Returns bytes sent.
    pub fn transmit(&mut self, kind: FrameKind, frame: &[u8], timestamp: u64) -> usize {
        match kind {
            FrameKind::Packet => {
                let n = self.port.send_packet(frame, timestamp);
                if let Some(ap) = &mut self.ap {
                    ap.send(frame);
                }
                n
            }
            FrameKind::Cmd => self.port.send_cmd(frame, timestamp),
            FrameKind::Reply { aid } => self.port.send_reply(frame, timestamp, aid),
            FrameKind::Ack => self.port.send_ack(frame, timestamp),
        }
    }

    /// Next received frame (12-byte RX header + 802.11 frame) and its
    /// timestamp (0 for AP frames), or `None`.
    pub fn receive(&mut self, buf: &mut [u8]) -> Option<(usize, u64)> {
        if let Recv::Frame { len, timestamp } = self.port.recv_packet(buf) {
            return Some((len, timestamp));
        }
        let len = self.ap.as_mut()?.recv(buf);
        (len > 0).then_some((len, 0))
    }
}
