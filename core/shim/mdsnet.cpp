// The C ABI in mdsnet.h, over melonDS's net layer. Compiled with
// -DmelonDS=melonDSnet: every `melonDS::` below is the renamed copy.

#include <algorithm>
#include <cstring>
#include <memory>
#include <mutex>
#include <optional>

#include "mdsnet.h"

#include "MPInterface.h"
#include "LAN.h"
#include "Net.h"
#include "Net_Slirp.h"
#include "Net_PCap.h"
#include "WifiAP.h"
#include "Wifi.h"

using namespace melonDS;

// melonDS's receive paths write straight into the caller's buffer with no
// bound beyond the frame size; these are the sizes the core itself uses.
static constexpr int kRecvScratch = 0x1000;
static constexpr int kReplyScratch = 16 * 1024 + 0x1000;

static int CopyOut(const u8* src, int len, u8* dst, int cap)
{
    if (len <= 0) return len;
    int n = std::min(len, cap);
    if (n > 0) memcpy(dst, src, n);
    return n;
}

// ---- MP --------------------------------------------------------------------

extern "C" void mdsnet_mp_set(int type) { MPInterface::Set((MPInterfaceType)type); }
extern "C" int mdsnet_mp_type(void) { return (int)MPInterface::GetType(); }
extern "C" void mdsnet_mp_set_recv_timeout(int ms) { MPInterface::Get().SetRecvTimeout(ms); }
extern "C" int mdsnet_mp_recv_timeout(void) { return MPInterface::Get().GetRecvTimeout(); }
extern "C" void mdsnet_mp_process(void) { MPInterface::Get().Process(); }
extern "C" void mdsnet_mp_begin(int inst) { MPInterface::Get().Begin(inst); }
extern "C" void mdsnet_mp_end(int inst) { MPInterface::Get().End(inst); }

extern "C" int mdsnet_mp_send_packet(int inst, const uint8_t* data, int len, uint64_t ts)
{
    return MPInterface::Get().SendPacket(inst, const_cast<u8*>(data), len, ts);
}
extern "C" int mdsnet_mp_send_cmd(int inst, const uint8_t* data, int len, uint64_t ts)
{
    return MPInterface::Get().SendCmd(inst, const_cast<u8*>(data), len, ts);
}
extern "C" int mdsnet_mp_send_reply(int inst, const uint8_t* data, int len, uint64_t ts, uint16_t aid)
{
    return MPInterface::Get().SendReply(inst, const_cast<u8*>(data), len, ts, aid);
}
extern "C" int mdsnet_mp_send_ack(int inst, const uint8_t* data, int len, uint64_t ts)
{
    return MPInterface::Get().SendAck(inst, const_cast<u8*>(data), len, ts);
}

extern "C" int mdsnet_mp_recv_packet(int inst, uint8_t* buf, int cap, uint64_t* ts)
{
    u8 tmp[kRecvScratch];
    u64 t = 0;
    int r = MPInterface::Get().RecvPacket(inst, tmp, &t);
    if (ts) *ts = t;
    return CopyOut(tmp, r, buf, cap);
}
extern "C" int mdsnet_mp_recv_host_packet(int inst, uint8_t* buf, int cap, uint64_t* ts)
{
    u8 tmp[kRecvScratch];
    u64 t = 0;
    int r = MPInterface::Get().RecvHostPacket(inst, tmp, &t);
    if (ts) *ts = t;
    return CopyOut(tmp, r, buf, cap);
}
extern "C" uint16_t mdsnet_mp_recv_replies(int inst, uint8_t* buf, int cap, uint64_t ts, uint16_t aidmask)
{
    auto tmp = std::make_unique<u8[]>(kReplyScratch);
    memset(tmp.get(), 0, kReplyScratch);
    u16 r = MPInterface::Get().RecvReplies(inst, tmp.get(), ts, aidmask);
    if (r) CopyOut(tmp.get(), std::min(cap, 15 * 1024), buf, cap);
    return r;
}

// ---- LAN -------------------------------------------------------------------

static LAN* CurrentLAN()
{
    if (MPInterface::GetType() != MPInterface_LAN) return nullptr;
    return static_cast<LAN*>(&MPInterface::Get());
}

extern "C" int mdsnet_lan_start_discovery(void) { auto l = CurrentLAN(); return l && l->StartDiscovery(); }
extern "C" void mdsnet_lan_end_discovery(void) { if (auto l = CurrentLAN()) l->EndDiscovery(); }
extern "C" int mdsnet_lan_start_host(const char* player, int max)
{
    auto l = CurrentLAN();
    return l && l->StartHost(player, max);
}
extern "C" int mdsnet_lan_start_client(const char* player, const char* host)
{
    auto l = CurrentLAN();
    return l && l->StartClient(player, host);
}
extern "C" void mdsnet_lan_end_session(void) { if (auto l = CurrentLAN()) l->EndSession(); }
extern "C" int mdsnet_lan_num_players(void) { auto l = CurrentLAN(); return l ? l->GetNumPlayers() : 0; }
extern "C" int mdsnet_lan_max_players(void) { auto l = CurrentLAN(); return l ? l->GetMaxPlayers() : 0; }

extern "C" int mdsnet_lan_discovery_list(MdsNetLanDiscovery* out, int cap)
{
    auto l = CurrentLAN();
    if (!l) return 0;
    auto list = l->GetDiscoveryList();
    int i = 0;
    for (auto& [addr, d] : list)
    {
        if (i < cap)
        {
            MdsNetLanDiscovery& o = out[i];
            memset(&o, 0, sizeof(o));
            o.address = addr;
            strncpy(o.session_name, d.SessionName, sizeof(o.session_name) - 1);
            o.num_players = d.NumPlayers;
            o.max_players = d.MaxPlayers;
            o.status = d.Status;
        }
        i++;
    }
    return i;
}

extern "C" int mdsnet_lan_player_list(MdsNetLanPlayer* out, int cap)
{
    auto l = CurrentLAN();
    if (!l) return 0;
    auto list = l->GetPlayerList();
    int i = 0;
    for (auto& p : list)
    {
        if (i < cap)
        {
            MdsNetLanPlayer& o = out[i];
            memset(&o, 0, sizeof(o));
            o.id = p.ID;
            strncpy(o.name, p.Name, sizeof(o.name) - 1);
            o.status = p.Status;
            o.address = p.Address;
            o.is_local = p.IsLocalPlayer;
            o.ping = p.Ping;
        }
        i++;
    }
    return i;
}

// ---- Net -------------------------------------------------------------------

struct MdsNet
{
    Net net;
    std::optional<LibPCap> pcap;
};

extern "C" MdsNet* mdsnet_net_new(void) { return new MdsNet(); }
extern "C" void mdsnet_net_free(MdsNet* n) { delete n; }

extern "C" int mdsnet_net_use_slirp(MdsNet* n)
{
    Net* net = &n->net;
    net->SetDriver(std::make_unique<Net_Slirp>([net](const u8* data, int len) { net->RXEnqueue(data, len); }));
    return 1;
}

static bool LoadPCap(MdsNet* n)
{
    if (!n->pcap) n->pcap = LibPCap::New();
    return n->pcap.has_value();
}

extern "C" int mdsnet_net_use_pcap(MdsNet* n, const char* device)
{
    if (!LoadPCap(n)) return 0;
    Net* net = &n->net;
    auto drv = n->pcap->Open(std::string_view(device),
        [net](const u8* data, int len) { net->RXEnqueue(data, len); });
    if (!drv) return 0;
    net->SetDriver(std::move(drv));
    return 1;
}

extern "C" void mdsnet_net_clear_driver(MdsNet* n) { n->net.SetDriver(nullptr); }
extern "C" int mdsnet_net_has_driver(const MdsNet* n) { return n->net.GetDriver() != nullptr; }

extern "C" int mdsnet_pcap_adapters(MdsNetAdapter* out, int cap)
{
    auto pcap = LibPCap::New();
    if (!pcap) return -1;
    auto list = pcap->GetAdapters();
    int i = 0;
    for (auto& a : list)
    {
        if (i < cap)
        {
            MdsNetAdapter& o = out[i];
            memcpy(o.device_name, a.DeviceName, sizeof(o.device_name));
            memcpy(o.friendly_name, a.FriendlyName, sizeof(o.friendly_name));
            memcpy(o.description, a.Description, sizeof(o.description));
            memcpy(o.mac, a.MAC, 6);
            memcpy(o.ipv4, a.IP_v4, 4);
        }
        i++;
    }
    return i;
}

extern "C" void mdsnet_net_register_instance(MdsNet* n, int inst) { n->net.RegisterInstance(inst); }
extern "C" void mdsnet_net_unregister_instance(MdsNet* n, int inst) { n->net.UnregisterInstance(inst); }

extern "C" int mdsnet_net_send_packet(MdsNet* n, int inst, const uint8_t* data, int len)
{
    return n->net.SendPacket(const_cast<u8*>(data), len, inst);
}

extern "C" int mdsnet_net_recv_packet(MdsNet* n, int inst, uint8_t* buf, int cap)
{
    // The dispatcher's queues hold frames up to 0x8000 bytes.
    auto tmp = std::make_unique<u8[]>(0x8000);
    int r = n->net.RecvPacket(tmp.get(), inst);
    return CopyOut(tmp.get(), r, buf, cap);
}

// ---- WifiAP ----------------------------------------------------------------

// WifiAP talks to its client through two narrow seams: `Wifi::GetMAC()` (to
// filter Ethernet frames by destination) and `Platform::Net_*` with the
// userdata it was built with. MdsNetAP stands in for both: `mac` is its first
// member, and it is what `Wifi::GetMAC` below reads through the `Wifi*`
// handed to the AP. No real Wifi object exists in this library.
struct MdsNetAP
{
    u8 mac[6];
    MdsNet* net;
    int inst;
    std::unique_ptr<WifiAP> ap;
};

const u8* melonDS::Wifi::GetMAC() const { return reinterpret_cast<const MdsNetAP*>(this)->mac; }

namespace melonDS
{
// Defined in Wifi.cpp, which this library does not build.
bool MACEqual(const u8* a, const u8* b) { return memcmp(a, b, 6) == 0; }
bool MACIsBroadcast(const u8* a) { return (a[0] & 0x01) != 0; }

namespace Platform
{
int Net_SendPacket(u8* data, int len, void* userdata)
{
    auto ap = static_cast<MdsNetAP*>(userdata);
    return ap && ap->net ? ap->net->net.SendPacket(data, len, ap->inst) : 0;
}

int Net_RecvPacket(u8* data, void* userdata)
{
    auto ap = static_cast<MdsNetAP*>(userdata);
    return ap && ap->net ? ap->net->net.RecvPacket(data, ap->inst) : 0;
}
}
}

extern "C" MdsNetAP* mdsnet_ap_new(MdsNet* net, int inst, const uint8_t mac[6])
{
    auto a = new MdsNetAP();
    memcpy(a->mac, mac, 6);
    a->net = net;
    a->inst = inst;
    a->ap = std::make_unique<WifiAP>(reinterpret_cast<Wifi*>(a), a);
    a->ap->Reset();
    if (net) net->net.RegisterInstance(inst);
    return a;
}

extern "C" void mdsnet_ap_free(MdsNetAP* a)
{
    if (!a) return;
    if (a->net) a->net->net.UnregisterInstance(a->inst);
    delete a;
}

extern "C" void mdsnet_ap_set_mac(MdsNetAP* a, const uint8_t mac[6]) { memcpy(a->mac, mac, 6); }
extern "C" void mdsnet_ap_reset(MdsNetAP* a) { a->ap->Reset(); }
extern "C" void mdsnet_ap_ms_timer(MdsNetAP* a) { a->ap->MSTimer(); }
extern "C" int mdsnet_ap_send_packet(MdsNetAP* a, const uint8_t* data, int len) { return a->ap->SendPacket(data, len); }

extern "C" int mdsnet_ap_recv_packet(MdsNetAP* a, uint8_t* buf, int cap)
{
    u8 tmp[0x1000];
    int r = a->ap->RecvPacket(tmp);
    return CopyOut(tmp, r, buf, cap);
}

extern "C" const char* mdsnet_ap_name(void) { return WifiAP::APName; }
extern "C" void mdsnet_ap_mac(uint8_t out[6]) { memcpy(out, WifiAP::APMac, 6); }
extern "C" uint8_t mdsnet_ap_channel(void) { return WifiAP::APChannel; }
