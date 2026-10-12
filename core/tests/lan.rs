//! melonDS's LAN backend between two processes on this machine: this test
//! hosts, and re-runs its own binary as the client (`MDSNET_LAN_ROLE`).
//! The MP backend is a per-process singleton, so two processes are the only
//! honest way to exercise it.

use std::process::Command;
use std::time::{Duration, Instant};

use melonds_net::lan;
use melonds_net::mp::{self, MpBackend, MpPort, Recv};

const ROLE: &str = "MDSNET_LAN_ROLE";

fn wait_frame(port: &MpPort, want: &[u8], secs: u64) -> bool {
    let mut buf = vec![0u8; 0x1000];
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        mp::process();
        if let Recv::Frame { len, .. } = port.recv_packet(&mut buf) {
            if &buf[..len] == want {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Runs only as the spawned client.
#[test]
fn lan_client_role() {
    if std::env::var(ROLE).as_deref() != Ok("client") {
        return;
    }
    mp::set_backend(MpBackend::Lan);
    lan::join("client", "127.0.0.1").expect("join");
    let port = MpPort::begin(0);
    assert!(wait_frame(&port, b"ping-from-host", 10), "client: no ping");
    // Answer until the host has seen it.
    for _ in 0..200 {
        port.send_packet(b"pong-from-client", 2);
        mp::process();
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(port);
    lan::leave();
}

#[test]
fn lan_host_and_client_exchange_frames() {
    if std::env::var(ROLE).is_ok() {
        return;
    }
    mp::set_backend(MpBackend::Lan);
    lan::host("host", 2).expect("host (UDP 7064 free?)");
    let port = MpPort::begin(0);

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lan_client_role",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, "client")
        .spawn()
        .expect("spawn client");

    // Wait for the client to join, then ping until it answers.
    let deadline = Instant::now() + Duration::from_secs(10);
    let joined = || {
        lan::players()
            .iter()
            .any(|p| p.name == "client" && p.status == lan::PlayerStatus::Client)
    };
    while !joined() {
        mp::process();
        assert!(Instant::now() < deadline, "client never joined");
        std::thread::sleep(Duration::from_millis(10));
    }
    let players = lan::players();
    println!("players: {players:?}");
    assert_eq!(lan::num_players(), 2);

    let mut buf = vec![0u8; 0x1000];
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = false;
    while Instant::now() < deadline && !got {
        port.send_packet(b"ping-from-host", 1);
        for _ in 0..10 {
            mp::process();
            if let Recv::Frame { len, .. } = port.recv_packet(&mut buf) {
                got |= &buf[..len] == b"pong-from-client";
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let status = child.wait().expect("client exit");
    drop(port);
    lan::leave();
    assert!(got, "host: no pong");
    assert!(status.success(), "client failed");
}
