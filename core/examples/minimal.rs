//! The README's minimal example: one console with an AP and slirp.
use melonds_net::{Station, mp, net};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    melonds_net::init_logging();
    mp::set_backend(mp::MpBackend::Local);
    let link = Arc::new(net::Net::with_driver(&net::Driver::Slirp)?);

    let mut sta = Station::new(0, [0x00, 0x09, 0xBF, 0, 0, 1], Some(link));
    let mut rx = vec![0u8; melonds_net::RX_BUFFER_SIZE];
    let mut us: u64 = 0;

    loop {
        // …run one emulated frame; inside it, the Wi-Fi MAC calls
        //    sta.transmit(FrameKind::Packet, &tx, us) when it sends …
        while let Some((len, _ts)) = sta.receive(&mut rx) {
            // deliver rx[..len] into the emulated RX buffer
            let _ = &rx[..len];
        }
        us += 16_715; // ≈ one 59.8 Hz frame of W_US_COUNT
        sta.tick(us);
        mp::process();
        if emulator_stopped() {
            break;
        }
    }
    Ok(())
}

fn emulator_stopped() -> bool {
    true
}
