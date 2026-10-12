//! Local wireless (MP) transport: melonDS's `MPInterface`.
//!
//! The DS wireless hardware exchanges three kinds of frames during a
//! multiplayer round (GBATEK "DS Wifi Multiplay Master/Slave"): the host's
//! **CMD** frame, each client's **reply** (sent by hardware, slot keyed by
//! AID), and the host's **ACK**. Everything else (beacons, auth/assoc,
//! ordinary data) is a plain **packet**. An emulator's Wi-Fi module calls the
//! [`MpPort`] method matching the frame it is transmitting, and polls the
//! receive methods when its RX logic wants a frame.
//!
//! Frames are `12-byte TX header + 802.11 frame`, timestamps are the sender's
//! emulated Wi-Fi microsecond counter (`W_US_COUNT`), exactly as melonDS's own
//! `Wifi.cpp` uses them.

use std::sync::Mutex;

use crate::ffi;

/// Largest frame (TX header included) the transports carry.
pub const MAX_FRAME_SIZE: usize = 0x948;
/// Bytes per AID slot in a [`MpPort::recv_replies`] buffer.
pub const REPLY_SLOT: usize = 1024;
/// Size of a buffer that can hold every reply slot (AIDs 1..=15).
pub const REPLY_BUFFER_SIZE: usize = 15 * REPLY_SLOT;
/// Highest instance number (consoles per process / per session).
pub const MAX_INSTANCES: usize = 16;

/// Which transport carries MP frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpBackend {
    /// Frames go nowhere; every receive is empty.
    Dummy,
    /// Consoles inside this process share one in-memory medium.
    Local,
    /// Consoles in different processes/machines, over ENet (UDP 7064), with
    /// session discovery on UDP 7063. Control it through [`crate::lan`].
    Lan,
}

impl MpBackend {
    const fn raw(self) -> i32 {
        match self {
            Self::Dummy => -1,
            Self::Local => 0,
            Self::Lan => 1,
        }
    }
}

static SELECT: Mutex<()> = Mutex::new(());

/// Replace the process-wide MP backend.
///
/// Must not race with MP calls from emulator threads: select the backend
/// before starting consoles (or after stopping them).
pub fn set_backend(backend: MpBackend) {
    let _g = SELECT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    unsafe { ffi::mdsnet_mp_set(backend.raw()) }
}

/// The current backend.
pub fn backend() -> MpBackend {
    match unsafe { ffi::mdsnet_mp_type() } {
        0 => MpBackend::Local,
        1 => MpBackend::Lan,
        _ => MpBackend::Dummy,
    }
}

/// How long the blocking receives (`recv_host_packet`, `recv_replies`) wait,
/// in milliseconds. melonDS's default is 25.
pub fn set_recv_timeout(ms: i32) {
    unsafe { ffi::mdsnet_mp_set_recv_timeout(ms) }
}

/// See [`set_recv_timeout`].
pub fn recv_timeout() -> i32 {
    unsafe { ffi::mdsnet_mp_recv_timeout() }
}

/// Pump the backend; call once per emulated video frame (LAN needs it to
/// service ENet and discovery).
pub fn process() {
    unsafe { ffi::mdsnet_mp_process() }
}

/// What a receive produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recv {
    /// No frame available.
    Empty,
    /// A frame of this many bytes was written, sent at this timestamp.
    Frame { len: usize, timestamp: u64 },
    /// The MP host this console was talking to has left.
    HostGone,
}

impl Recv {
    const fn from_raw(r: i32, timestamp: u64) -> Self {
        match r {
            0 => Self::Empty,
            r if r < 0 => Self::HostGone,
            r => Self::Frame {
                len: r as usize,
                timestamp,
            },
        }
    }
}

/// A console's address on the MP medium, without lifetime management.
///
/// For hosts whose emulator core already signals Wi-Fi power on/off (like
/// melonDS's `Platform::MP_Begin`/`MP_End`): call [`MpHandle::begin`] and
/// [`MpHandle::end`] from those hooks. Otherwise prefer [`MpPort`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpHandle {
    inst: i32,
}

impl MpHandle {
    /// The handle of console `inst` (`0..MAX_INSTANCES`, unique per process).
    ///
    /// # Panics
    /// If `inst` is out of range.
    pub fn new(inst: usize) -> Self {
        assert!(inst < MAX_INSTANCES, "instance {inst} out of range");
        Self { inst: inst as i32 }
    }

    /// The instance number.
    pub const fn instance(self) -> usize {
        self.inst as usize
    }

    /// Join the medium (the game powered the Wi-Fi hardware on).
    pub fn begin(self) {
        unsafe { ffi::mdsnet_mp_begin(self.inst) }
    }

    /// Leave the medium.
    pub fn end(self) {
        unsafe { ffi::mdsnet_mp_end(self.inst) }
    }

    fn len(data: &[u8]) -> i32 {
        data.len().min(i32::MAX as usize) as i32
    }

    /// Send an ordinary frame (beacon, management, data). Returns bytes sent.
    pub fn send_packet(self, data: &[u8], timestamp: u64) -> usize {
        unsafe { ffi::mdsnet_mp_send_packet(self.inst, data.as_ptr(), Self::len(data), timestamp) }
            .max(0) as usize
    }

    /// Send a multiplay CMD frame (host side).
    pub fn send_cmd(self, data: &[u8], timestamp: u64) -> usize {
        unsafe { ffi::mdsnet_mp_send_cmd(self.inst, data.as_ptr(), Self::len(data), timestamp) }
            .max(0) as usize
    }

    /// Send a multiplay reply as client `aid` (client side). An empty `data`
    /// is a "no reply" marker, as melonDS's Wi-Fi sends when the reply
    /// window is missed.
    pub fn send_reply(self, data: &[u8], timestamp: u64, aid: u16) -> usize {
        let ptr = if data.is_empty() {
            std::ptr::null()
        } else {
            data.as_ptr()
        };
        unsafe { ffi::mdsnet_mp_send_reply(self.inst, ptr, Self::len(data), timestamp, aid) }.max(0)
            as usize
    }

    /// Send a multiplay ACK frame (host side).
    pub fn send_ack(self, data: &[u8], timestamp: u64) -> usize {
        unsafe { ffi::mdsnet_mp_send_ack(self.inst, data.as_ptr(), Self::len(data), timestamp) }
            .max(0) as usize
    }

    /// Non-blocking receive of any frame from another console.
    pub fn recv_packet(self, buf: &mut [u8]) -> Recv {
        let mut ts = 0;
        let r = unsafe {
            ffi::mdsnet_mp_recv_packet(self.inst, buf.as_mut_ptr(), Self::len(buf), &mut ts)
        };
        Recv::from_raw(r, ts)
    }

    /// Blocking (up to [`recv_timeout`]) receive used by a client waiting
    /// for the host's CMD/ACK.
    pub fn recv_host_packet(self, buf: &mut [u8]) -> Recv {
        let mut ts = 0;
        let r = unsafe {
            ffi::mdsnet_mp_recv_host_packet(self.inst, buf.as_mut_ptr(), Self::len(buf), &mut ts)
        };
        Recv::from_raw(r, ts)
    }

    /// Host side: collect the replies to the CMD sent at `timestamp` from the
    /// clients in `aidmask`. Reply `aid` lands at `buf[(aid-1)*REPLY_SLOT..]`
    /// (`buf` should be [`REPLY_BUFFER_SIZE`]). Returns the AIDs received.
    pub fn recv_replies(self, buf: &mut [u8], timestamp: u64, aidmask: u16) -> u16 {
        unsafe {
            ffi::mdsnet_mp_recv_replies(
                self.inst,
                buf.as_mut_ptr(),
                Self::len(buf),
                timestamp,
                aidmask,
            )
        }
    }
}

/// One console's connection to the MP medium: [`MpHandle::begin`] on
/// creation, [`MpHandle::end`] on drop. Derefs to [`MpHandle`] for the
/// send/receive calls.
#[derive(Debug)]
pub struct MpPort {
    handle: MpHandle,
}

impl MpPort {
    /// Join the medium as console `inst` (`0..MAX_INSTANCES`, unique per
    /// process).
    ///
    /// # Panics
    /// If `inst` is out of range.
    pub fn begin(inst: usize) -> Self {
        let handle = MpHandle::new(inst);
        handle.begin();
        Self { handle }
    }

    /// The underlying handle.
    pub const fn handle(&self) -> MpHandle {
        self.handle
    }
}

impl std::ops::Deref for MpPort {
    type Target = MpHandle;
    fn deref(&self) -> &MpHandle {
        &self.handle
    }
}

impl Drop for MpPort {
    fn drop(&mut self) {
        self.handle.end();
    }
}
