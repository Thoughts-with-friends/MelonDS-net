//! One full multiplay round over melonDS's LocalMP: beacon, CMD, reply, ACK,
//! between two consoles in this process.

use melonds_net::mp::{self, MpBackend, MpPort, REPLY_BUFFER_SIZE, REPLY_SLOT, Recv};

fn frame(tag: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| tag ^ i as u8).collect()
}

#[test]
fn cmd_reply_ack_round() {
    melonds_net::init_logging();
    mp::set_backend(MpBackend::Local);
    assert_eq!(mp::backend(), MpBackend::Local);

    let host = MpPort::begin(0);
    let client = MpPort::begin(1);
    let mut buf = vec![0u8; 0x1000];

    // A beacon reaches the other console, not the sender.
    let beacon = frame(0xB0, 64);
    assert_eq!(host.send_packet(&beacon, 1000), beacon.len());
    assert_eq!(host.recv_packet(&mut buf), Recv::Empty);
    assert_eq!(
        client.recv_packet(&mut buf),
        Recv::Frame {
            len: 64,
            timestamp: 1000
        }
    );
    assert_eq!(&buf[..64], &beacon[..]);

    // The host's CMD, answered from a client thread the way the client's
    // hardware does, while the host blocks on the replies.
    let cmd = frame(0xC0, 48);
    let ts = 5000;
    let answering = std::thread::spawn(move || {
        let mut buf = vec![0u8; 0x1000];
        let got = client.recv_host_packet(&mut buf);
        assert_eq!(
            got,
            Recv::Frame {
                len: 48,
                timestamp: ts
            }
        );
        client.send_reply(&frame(0xA1, 30), ts + 10, 1);
        let ack = client.recv_host_packet(&mut buf);
        assert_eq!(
            ack,
            Recv::Frame {
                len: 32,
                timestamp: ts + 100
            }
        );
        client
    });

    assert_eq!(host.send_cmd(&cmd, ts), cmd.len());
    let mut replies = vec![0u8; REPLY_BUFFER_SIZE];
    let mask = host.recv_replies(&mut replies, ts, 0b10);
    assert_eq!(mask, 0b10, "reply from AID 1");
    assert_eq!(&replies[..30], &frame(0xA1, 30)[..]);
    assert!(replies[REPLY_SLOT..].iter().all(|&b| b == 0));
    host.send_ack(&frame(0xAC, 32), ts + 100);

    let client = answering.join().expect("client thread");
    drop(client);
    drop(host);
}
