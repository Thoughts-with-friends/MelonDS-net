//! Infrastructure path end to end: a console authenticates and associates
//! with melonDS's emulated AP, then broadcasts a DHCP DISCOVER as an 802.11
//! data frame; libslirp (through Net) answers with a DHCP OFFER that comes
//! back out of the AP as an 802.11 data frame.

use std::sync::Arc;
use std::time::{Duration, Instant};

use melonds_net::ap::{self, WifiAp};
use melonds_net::net::{Driver, Net};

const DS_MAC: [u8; 6] = [0x00, 0x09, 0xBF, 0x11, 0x22, 0x33];

/// 12-byte TX header (byte 9 = channel, as melonDS's Wifi.cpp fills it)
/// followed by the 802.11 frame and a 4-byte FCS.
fn tx(frame: &[u8]) -> Vec<u8> {
    let mut v = vec![0u8; 12];
    v[9] = ap::channel();
    v.extend_from_slice(frame);
    v.extend_from_slice(&[0; 4]);
    v
}

fn mgmt(subtype: u16, body: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&(subtype << 4).to_le_bytes());
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&ap::mac()); // receiver
    f.extend_from_slice(&DS_MAC); // sender
    f.extend_from_slice(&ap::mac()); // BSSID
    f.extend_from_slice(&[0, 0]); // seq
    f.extend_from_slice(body);
    tx(&f)
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for c in data.chunks(2) {
        sum += u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]));
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// IPv4/UDP DHCPDISCOVER from 0.0.0.0:68 to 255.255.255.255:67.
fn dhcp_discover() -> Vec<u8> {
    let mut bootp = vec![0u8; 240];
    bootp[0] = 1; // BOOTREQUEST
    bootp[1] = 1; // ethernet
    bootp[2] = 6;
    bootp[4..8].copy_from_slice(&0x1234_5678u32.to_be_bytes()); // xid
    bootp[10] = 0x80; // broadcast flag
    bootp[28..34].copy_from_slice(&DS_MAC);
    bootp[236..240].copy_from_slice(&[99, 130, 83, 99]); // magic cookie
    bootp.extend_from_slice(&[53, 1, 1, 255]); // DHCPDISCOVER, end

    let udp_len = 8 + bootp.len();
    let mut udp = Vec::new();
    udp.extend_from_slice(&68u16.to_be_bytes());
    udp.extend_from_slice(&67u16.to_be_bytes());
    udp.extend_from_slice(&(udp_len as u16).to_be_bytes());
    udp.extend_from_slice(&[0, 0]); // no checksum
    udp.extend_from_slice(&bootp);

    let mut ip = vec![
        0x45, 0, 0, 0, 0, 1, 0, 0, 64, 17, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255,
    ];
    let total = (20 + udp.len()) as u16;
    ip[2..4].copy_from_slice(&total.to_be_bytes());
    let c = checksum(&ip);
    ip[10..12].copy_from_slice(&c.to_be_bytes());
    ip.extend_from_slice(&udp);
    ip
}

/// 802.11 data frame, ToDS, LLC/SNAP-encapsulated IPv4.
fn data_frame(dest: [u8; 6], payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(&0x0108u16.to_le_bytes()); // data, ToDS
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&ap::mac()); // BSSID
    f.extend_from_slice(&DS_MAC); // source
    f.extend_from_slice(&dest); // destination
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&[0xAA, 0xAA, 0x03, 0, 0, 0, 0x08, 0x00]);
    f.extend_from_slice(payload);
    tx(&f)
}

fn frame_ctl(rx: &[u8]) -> u16 {
    u16::from_le_bytes([rx[12], rx[13]])
}

#[test]
fn associate_and_get_dhcp_offer() {
    melonds_net::init_logging();
    let net = Arc::new(Net::with_driver(&Driver::Slirp).expect("slirp"));
    assert!(net.has_driver());
    let mut ap = WifiAp::new(Some(Arc::clone(&net)), 0, DS_MAC);
    let mut rx = vec![0u8; ap::RECV_BUFFER_SIZE];

    // Beacon after 128 ms of AP time.
    for _ in 0..128 {
        ap.ms_timer();
    }
    let n = ap.recv(&mut rx);
    assert!(n > 12 && frame_ctl(&rx) == 0x0080, "beacon");
    assert!(
        rx[..n].windows(7).any(|w| w == ap::name().as_bytes()),
        "SSID in beacon"
    );

    // Open-system authentication, then association.
    assert!(ap.send(&mgmt(0xB, &[0, 0, 1, 0, 0, 0])) > 0);
    let n = ap.recv(&mut rx);
    assert!(n > 0 && frame_ctl(&rx) == 0x00B0, "auth response");
    assert!(ap.send(&mgmt(0x0, &[0x21, 0, 1, 0])) > 0);
    let n = ap.recv(&mut rx);
    assert!(n > 0 && frame_ctl(&rx) == 0x0010, "assoc response");

    // DHCP over the bridge.
    assert!(ap.send(&data_frame([0xFF; 6], &dhcp_discover())) > 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    let offer = loop {
        let n = ap.recv(&mut rx);
        if n > 0 && frame_ctl(&rx) == 0x0208 {
            break rx[12..n].to_vec();
        }
        assert!(Instant::now() < deadline, "no DHCP offer from slirp");
        std::thread::sleep(Duration::from_millis(5));
    };

    // 24-byte 802.11 header + 8-byte LLC/SNAP, then IPv4/UDP/BOOTP.
    let ip = &offer[32..];
    assert_eq!(ip[9], 17, "UDP");
    let udp = &ip[20..];
    assert_eq!(
        u16::from_be_bytes([udp[0], udp[1]]),
        67,
        "from the DHCP server"
    );
    let bootp = &udp[8..];
    assert_eq!(bootp[0], 2, "BOOTREPLY");
    assert_eq!(&bootp[4..8], &0x1234_5678u32.to_be_bytes(), "our xid");
    let yiaddr = &bootp[16..20];
    assert_eq!(yiaddr, &[10, 64, 0, 16], "melonDS slirp lease");
    println!(
        "DHCP offer: {}.{}.{}.{}",
        yiaddr[0], yiaddr[1], yiaddr[2], yiaddr[3]
    );
}
