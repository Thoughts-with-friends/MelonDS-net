//! Raw declarations for `shim/mdsnet.h`. See that header for the contract of
//! every function; the safe wrappers live in the sibling modules.
#![allow(missing_docs)]

use std::ffi::{c_char, c_int};

pub type MdsNetLogFn = unsafe extern "C" fn(level: c_int, msg: *const c_char);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MdsNetLanDiscovery {
    pub address: u32,
    pub session_name: [c_char; 64],
    pub num_players: u8,
    pub max_players: u8,
    pub status: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MdsNetLanPlayer {
    pub id: i32,
    pub name: [c_char; 32],
    pub status: i32,
    pub address: u32,
    pub is_local: u8,
    pub ping: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MdsNetAdapter {
    pub device_name: [c_char; 128],
    pub friendly_name: [c_char; 128],
    pub description: [c_char; 128],
    pub mac: [u8; 6],
    pub ipv4: [u8; 4],
}

#[repr(C)]
pub struct MdsNet {
    _opaque: [u8; 0],
}

#[repr(C)]
pub struct MdsNetAP {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    pub fn mdsnet_set_log(f: Option<MdsNetLogFn>);

    pub fn mdsnet_mp_set(kind: c_int);
    pub fn mdsnet_mp_type() -> c_int;
    pub fn mdsnet_mp_set_recv_timeout(ms: c_int);
    pub fn mdsnet_mp_recv_timeout() -> c_int;
    pub fn mdsnet_mp_process();
    pub fn mdsnet_mp_begin(inst: c_int);
    pub fn mdsnet_mp_end(inst: c_int);
    pub fn mdsnet_mp_send_packet(inst: c_int, data: *const u8, len: c_int, ts: u64) -> c_int;
    pub fn mdsnet_mp_send_cmd(inst: c_int, data: *const u8, len: c_int, ts: u64) -> c_int;
    pub fn mdsnet_mp_send_reply(
        inst: c_int,
        data: *const u8,
        len: c_int,
        ts: u64,
        aid: u16,
    ) -> c_int;
    pub fn mdsnet_mp_send_ack(inst: c_int, data: *const u8, len: c_int, ts: u64) -> c_int;
    pub fn mdsnet_mp_recv_packet(inst: c_int, buf: *mut u8, cap: c_int, ts: *mut u64) -> c_int;
    pub fn mdsnet_mp_recv_host_packet(inst: c_int, buf: *mut u8, cap: c_int, ts: *mut u64)
    -> c_int;
    pub fn mdsnet_mp_recv_replies(
        inst: c_int,
        buf: *mut u8,
        cap: c_int,
        ts: u64,
        aidmask: u16,
    ) -> u16;

    pub fn mdsnet_lan_start_discovery() -> c_int;
    pub fn mdsnet_lan_end_discovery();
    pub fn mdsnet_lan_start_host(player: *const c_char, max_players: c_int) -> c_int;
    pub fn mdsnet_lan_start_client(player: *const c_char, host: *const c_char) -> c_int;
    pub fn mdsnet_lan_end_session();
    pub fn mdsnet_lan_discovery_list(out: *mut MdsNetLanDiscovery, cap: c_int) -> c_int;
    pub fn mdsnet_lan_player_list(out: *mut MdsNetLanPlayer, cap: c_int) -> c_int;
    pub fn mdsnet_lan_num_players() -> c_int;
    pub fn mdsnet_lan_max_players() -> c_int;

    pub fn mdsnet_net_new() -> *mut MdsNet;
    pub fn mdsnet_net_free(net: *mut MdsNet);
    pub fn mdsnet_net_use_slirp(net: *mut MdsNet) -> c_int;
    pub fn mdsnet_net_use_pcap(net: *mut MdsNet, device: *const c_char) -> c_int;
    pub fn mdsnet_net_clear_driver(net: *mut MdsNet);
    pub fn mdsnet_net_has_driver(net: *const MdsNet) -> c_int;
    pub fn mdsnet_pcap_adapters(out: *mut MdsNetAdapter, cap: c_int) -> c_int;
    pub fn mdsnet_net_register_instance(net: *mut MdsNet, inst: c_int);
    pub fn mdsnet_net_unregister_instance(net: *mut MdsNet, inst: c_int);
    pub fn mdsnet_net_send_packet(
        net: *mut MdsNet,
        inst: c_int,
        data: *const u8,
        len: c_int,
    ) -> c_int;
    pub fn mdsnet_net_recv_packet(net: *mut MdsNet, inst: c_int, buf: *mut u8, cap: c_int)
    -> c_int;

    pub fn mdsnet_ap_new(net: *mut MdsNet, inst: c_int, mac: *const u8) -> *mut MdsNetAP;
    pub fn mdsnet_ap_free(ap: *mut MdsNetAP);
    pub fn mdsnet_ap_set_mac(ap: *mut MdsNetAP, mac: *const u8);
    pub fn mdsnet_ap_reset(ap: *mut MdsNetAP);
    pub fn mdsnet_ap_ms_timer(ap: *mut MdsNetAP);
    pub fn mdsnet_ap_send_packet(ap: *mut MdsNetAP, data: *const u8, len: c_int) -> c_int;
    pub fn mdsnet_ap_recv_packet(ap: *mut MdsNetAP, buf: *mut u8, cap: c_int) -> c_int;
    pub fn mdsnet_ap_name() -> *const c_char;
    pub fn mdsnet_ap_mac(out: *mut u8);
    pub fn mdsnet_ap_channel() -> u8;
}
