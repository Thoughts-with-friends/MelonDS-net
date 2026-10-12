/*
    melonds_net: a C ABI over melonDS's network layer.

    Everything an emulator needs to put an emulated DS on a network, taken
    from melonDS without its console core:

      - MP (local wireless / "DS Wireless Play") transports:
        melonDS's LocalMP (consoles in one process) and LAN (ENet over UDP,
        with discovery), behind the MPInterface singleton.
      - Host networking for infrastructure mode (Nintendo WFC and friends):
        Net + a driver (libslirp user-mode NAT, or libpcap bridging).
      - WifiAP: melonDS's emulated access point, translating between the
        DS's 802.11 frames and the Ethernet frames Net carries.

    Frames on the MP and AP paths are in melonDS's wire format: a 12-byte
    TX header (the Wi-Fi RAM TX header, GBATEK "DS Wifi Transmit Buffers")
    followed by the raw 802.11 frame. Timestamps are the sender's emulated
    Wi-Fi microsecond counter.

    The melonDS sources behind this header are compiled with their namespace
    renamed (melonDS -> melonDSnet), so this library links cleanly next to a
    full melonDS core, or any other emulator.

    License: GPL-3.0-or-later (melonDS's license governs the combined work).
*/
#ifndef MDSNET_H
#define MDSNET_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- logging ---------------------------------------------------------- */

/* level: 0=debug 1=info 2=warn 3=error. msg is NUL-terminated. */
typedef void (*MdsNetLogFn)(int level, const char* msg);
void mdsnet_set_log(MdsNetLogFn fn);

/* ---- MP transport ----------------------------------------------------- */

#define MDSNET_MP_DUMMY (-1)
#define MDSNET_MP_LOCAL 0
#define MDSNET_MP_LAN 1

/* Replace the process-wide MP backend. No instance may be inside an MP call. */
void mdsnet_mp_set(int type);
int mdsnet_mp_type(void);
void mdsnet_mp_set_recv_timeout(int ms);
int mdsnet_mp_recv_timeout(void);
/* Once per emulated video frame (LAN: pumps ENet + discovery). */
void mdsnet_mp_process(void);

void mdsnet_mp_begin(int inst);
void mdsnet_mp_end(int inst);

/* Sends return the bytes sent (0 = dropped). */
int mdsnet_mp_send_packet(int inst, const uint8_t* data, int len, uint64_t timestamp);
int mdsnet_mp_send_cmd(int inst, const uint8_t* data, int len, uint64_t timestamp);
int mdsnet_mp_send_reply(int inst, const uint8_t* data, int len, uint64_t timestamp, uint16_t aid);
int mdsnet_mp_send_ack(int inst, const uint8_t* data, int len, uint64_t timestamp);

/* Receives return the frame length, 0 = nothing, -1 = host gone.
   A frame longer than cap is truncated to cap. */
int mdsnet_mp_recv_packet(int inst, uint8_t* buf, int cap, uint64_t* timestamp);
int mdsnet_mp_recv_host_packet(int inst, uint8_t* buf, int cap, uint64_t* timestamp);
/* Replies land at buf[(aid-1)*1024]; returns the bitmask of AIDs written. */
uint16_t mdsnet_mp_recv_replies(int inst, uint8_t* buf, int cap, uint64_t timestamp, uint16_t aidmask);

/* ---- LAN session (valid while the backend is MDSNET_MP_LAN) ---------- */

typedef struct MdsNetLanDiscovery {
    uint32_t address; /* IPv4 as in_addr.s_addr */
    char session_name[64];
    uint8_t num_players;
    uint8_t max_players;
    uint8_t status; /* 0=idle 1=playing */
} MdsNetLanDiscovery;

typedef struct MdsNetLanPlayer {
    int32_t id;
    char name[32];
    int32_t status; /* 0=none 1=client 2=host 3=connecting 4=disconnected */
    uint32_t address;
    uint8_t is_local;
    uint32_t ping;
} MdsNetLanPlayer;

int mdsnet_lan_start_discovery(void);
void mdsnet_lan_end_discovery(void);
int mdsnet_lan_start_host(const char* player, int max_players);
/* Blocks up to ~5 s while connecting. */
int mdsnet_lan_start_client(const char* player, const char* host);
void mdsnet_lan_end_session(void);
/* Write up to cap entries, return how many exist. */
int mdsnet_lan_discovery_list(MdsNetLanDiscovery* out, int cap);
int mdsnet_lan_player_list(MdsNetLanPlayer* out, int cap);
int mdsnet_lan_num_players(void);
int mdsnet_lan_max_players(void);

/* ---- host networking -------------------------------------------------- */

typedef struct MdsNet MdsNet;

MdsNet* mdsnet_net_new(void);
void mdsnet_net_free(MdsNet* net);
/* Select a driver. Return 1 on success. */
int mdsnet_net_use_slirp(MdsNet* net);
int mdsnet_net_use_pcap(MdsNet* net, const char* device);
void mdsnet_net_clear_driver(MdsNet* net);
int mdsnet_net_has_driver(const MdsNet* net);

typedef struct MdsNetAdapter {
    char device_name[128];
    char friendly_name[128];
    char description[128];
    uint8_t mac[6];
    uint8_t ipv4[4];
} MdsNetAdapter;
/* -1 if libpcap/Npcap is not installed. */
int mdsnet_pcap_adapters(MdsNetAdapter* out, int cap);

void mdsnet_net_register_instance(MdsNet* net, int inst);
void mdsnet_net_unregister_instance(MdsNet* net, int inst);
/* Ethernet frames. */
int mdsnet_net_send_packet(MdsNet* net, int inst, const uint8_t* data, int len);
int mdsnet_net_recv_packet(MdsNet* net, int inst, uint8_t* buf, int cap);

/* ---- emulated access point ------------------------------------------- */

typedef struct MdsNetAP MdsNetAP;

/* net may be NULL (AP beacons and associates, but carries no traffic). */
MdsNetAP* mdsnet_ap_new(MdsNet* net, int inst, const uint8_t mac[6]);
void mdsnet_ap_free(MdsNetAP* ap);
void mdsnet_ap_set_mac(MdsNetAP* ap, const uint8_t mac[6]);
void mdsnet_ap_reset(MdsNetAP* ap);
/* Every emulated millisecond (beacons every 128 ms). */
void mdsnet_ap_ms_timer(MdsNetAP* ap);
/* TX header + 802.11 frame in; return len consumed (0 = not for the AP). */
int mdsnet_ap_send_packet(MdsNetAP* ap, const uint8_t* data, int len);
/* RX header + 802.11 frame out; return len, 0 = nothing. */
int mdsnet_ap_recv_packet(MdsNetAP* ap, uint8_t* buf, int cap);

const char* mdsnet_ap_name(void);
void mdsnet_ap_mac(uint8_t out[6]);
uint8_t mdsnet_ap_channel(void);

#ifdef __cplusplus
}
#endif

#endif
