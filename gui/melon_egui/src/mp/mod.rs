//! Local wireless for the consoles in this window.
//!
//! The medium is [`melonds_net`]: melonDS's `MPInterface` compiled from the
//! `melonDS` submodule and called over FFI. By default it is melonDS's
//! LocalMP (both consoles of this window share it); `--lan-host` /
//! `--lan-join <ip>` switch it to melonDS's LAN backend so a console in
//! another process or on another machine is on the same air.
//!
//! What lives here is only the adapter ([`Client`], one per console) and the
//! counters/log the wireless status pane reads ([`Airwaves`]).

use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

mod airwaves;
mod client;
mod counters;
mod frame;

pub use airwaves::Airwaves;
pub use client::Client;
pub use counters::Counters;
pub(crate) use frame::LOG_LIMIT;
pub use frame::{Event, Kind};

#[cfg(test)]
mod tests {
    use melonds::Host;

    use super::Airwaves;

    /// A CMD from console 0 reaches console 1 through melonDS's LocalMP, and
    /// the counters the status pane shows follow it.
    #[test]
    fn a_cmd_crosses_melonds_local_mp() {
        let air = Airwaves::new();
        let (a, b) = (air.client(0), air.client(1));
        a.mp_begin();
        b.mp_begin();
        assert_eq!(a.mp_send_cmd(b"round", 2_000), 5);
        let mut buf = [0u8; 64];
        let mut ts = 0;
        assert_eq!(b.mp_recv_host_packet(&mut buf, 0, &mut ts), Some(5));
        assert_eq!((&buf[..5], ts), (&b"round"[..], 2_000));
        assert_eq!(air.counters()[0].sent_cmd, 1);
        assert_eq!(air.counters()[1].recv_cmd, 1);
        a.mp_end();
        b.mp_end();
    }
}
