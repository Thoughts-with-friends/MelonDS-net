//! LAN sessions: melonDS's `LAN` MP backend (ENet on UDP 7064, discovery
//! broadcasts on UDP 7063). Only meaningful while
//! [`crate::mp::backend`] is [`crate::mp::MpBackend::Lan`].
//!
//! Typical flow: `mp::set_backend(Lan)`, then either [`host`] or
//! [`join`], start the console(s), and call [`crate::mp::process`] every
//! frame.

use std::ffi::{CStr, CString, c_char};
use std::net::Ipv4Addr;

use crate::ffi;

/// Errors from session control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanError {
    /// The MP backend is not [`crate::mp::MpBackend::Lan`], or ENet failed
    /// to initialise.
    NotLan,
    /// A name contained a NUL byte.
    BadString,
    /// Binding / connecting failed (port in use, host unreachable, timeout).
    Failed,
}

impl std::fmt::Display for LanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotLan => "the MP backend is not LAN",
            Self::BadString => "string contains NUL",
            Self::Failed => "LAN session could not be started",
        })
    }
}

impl std::error::Error for LanError {}

fn cstr(s: &str) -> Result<CString, LanError> {
    CString::new(s).map_err(|_| LanError::BadString)
}

fn lan_active() -> Result<(), LanError> {
    (crate::mp::backend() == crate::mp::MpBackend::Lan)
        .then_some(())
        .ok_or(LanError::NotLan)
}

fn ok(r: i32) -> Result<(), LanError> {
    (r != 0).then_some(()).ok_or(LanError::Failed)
}

/// Host a session for up to `max_players` (≤ 16) consoles. Also starts
/// answering discovery.
///
/// # Errors
/// [`LanError`] if the backend is not LAN or the port could not be bound.
pub fn host(player: &str, max_players: u8) -> Result<(), LanError> {
    lan_active()?;
    let p = cstr(player)?;
    ok(unsafe { ffi::mdsnet_lan_start_host(p.as_ptr(), i32::from(max_players)) })
}

/// Join the session hosted at `host` (IP or host name). Blocks up to ~5 s.
///
/// # Errors
/// [`LanError`] if the backend is not LAN or the host did not accept.
pub fn join(player: &str, host: &str) -> Result<(), LanError> {
    lan_active()?;
    let (p, h) = (cstr(player)?, cstr(host)?);
    ok(unsafe { ffi::mdsnet_lan_start_client(p.as_ptr(), h.as_ptr()) })
}

/// Leave/close the current session.
pub fn leave() {
    unsafe { ffi::mdsnet_lan_end_session() }
}

/// Start listening for hosts' discovery beacons (see [`discovered`]).
///
/// # Errors
/// [`LanError`] if the backend is not LAN or UDP 7063 is taken.
pub fn start_discovery() -> Result<(), LanError> {
    lan_active()?;
    ok(unsafe { ffi::mdsnet_lan_start_discovery() })
}

/// Stop listening for discovery beacons.
pub fn end_discovery() {
    unsafe { ffi::mdsnet_lan_end_discovery() }
}

/// A session found by discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub address: Ipv4Addr,
    pub session_name: String,
    pub num_players: u8,
    pub max_players: u8,
    pub playing: bool,
}

/// A player in the current session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    pub id: i32,
    pub name: String,
    pub status: PlayerStatus,
    pub address: Ipv4Addr,
    pub is_local: bool,
    pub ping_ms: u32,
}

/// [`Player::status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStatus {
    None,
    Client,
    Host,
    Connecting,
    Disconnected,
}

fn text(raw: &[c_char]) -> String {
    let bytes: Vec<u8> = raw
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    CStr::from_bytes_until_nul(&[bytes.as_slice(), &[0]].concat())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// melonDS stores addresses as `in_addr.s_addr` (network order in memory).
fn addr(raw: u32) -> Ipv4Addr {
    Ipv4Addr::from(raw.to_ne_bytes())
}

fn fetch<T: Copy>(f: unsafe extern "C" fn(*mut T, i32) -> i32, zero: T) -> Vec<T> {
    let n = unsafe { f(std::ptr::null_mut(), 0) }.max(0) as usize;
    let mut v = vec![zero; n];
    let got = unsafe { f(v.as_mut_ptr(), n as i32) }.max(0) as usize;
    v.truncate(got.min(n));
    v
}

/// Sessions seen since [`start_discovery`].
pub fn discovered() -> Vec<Discovered> {
    let zero = ffi::MdsNetLanDiscovery {
        address: 0,
        session_name: [0; 64],
        num_players: 0,
        max_players: 0,
        status: 0,
    };
    fetch(ffi::mdsnet_lan_discovery_list, zero)
        .into_iter()
        .map(|d| Discovered {
            address: addr(d.address),
            session_name: text(&d.session_name),
            num_players: d.num_players,
            max_players: d.max_players,
            playing: d.status != 0,
        })
        .collect()
}

/// Players in the current session.
pub fn players() -> Vec<Player> {
    let zero = ffi::MdsNetLanPlayer {
        id: 0,
        name: [0; 32],
        status: 0,
        address: 0,
        is_local: 0,
        ping: 0,
    };
    fetch(ffi::mdsnet_lan_player_list, zero)
        .into_iter()
        .map(|p| Player {
            id: p.id,
            name: text(&p.name),
            status: match p.status {
                1 => PlayerStatus::Client,
                2 => PlayerStatus::Host,
                3 => PlayerStatus::Connecting,
                4 => PlayerStatus::Disconnected,
                _ => PlayerStatus::None,
            },
            address: addr(p.address),
            is_local: p.is_local != 0,
            ping_ms: p.ping,
        })
        .collect()
}

/// Players currently in the session.
pub fn num_players() -> usize {
    unsafe { ffi::mdsnet_lan_num_players() }.max(0) as usize
}

/// The session's player limit.
pub fn max_players() -> usize {
    unsafe { ffi::mdsnet_lan_max_players() }.max(0) as usize
}
