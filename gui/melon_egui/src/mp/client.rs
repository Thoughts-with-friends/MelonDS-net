//! One console's seat on the air.
//!
//! The frames themselves travel through [`melonds_net`] — melonDS's own
//! `MPInterface` (LocalMP in this process, or LAN to other machines) built
//! from the `melonDS` submodule and called over FFI. This type only adapts
//! the `melonds::Host` hooks to it and keeps the counters and frame log the
//! wireless status pane shows.

use melonds_net::mp::{MpHandle, Recv};

use super::*;

/// A console's seat: instance number plus the shared bookkeeping.
#[derive(Clone)]
pub struct Client {
    pub(crate) airwaves: Airwaves,
    pub(crate) instance: usize,
}

impl Client {
    fn port(&self) -> MpHandle {
        MpHandle::new(self.instance)
    }

    pub(crate) fn begin(&self) {
        self.port().begin();
        self.airwaves.0.0.lock().unwrap().boxes[self.instance].connected = true;
    }

    pub(crate) fn end(&self) {
        self.port().end();
        let mut shared = self.airwaves.0.0.lock().unwrap();
        let mailbox = &mut shared.boxes[self.instance];
        mailbox.connected = false;
        mailbox.last_host = None;
    }

    /// Record a frame this console sent (counters + log).
    fn sent(&self, kind: Kind, len: usize, timestamp: u64) {
        let mut shared = self.airwaves.0.0.lock().unwrap();
        let counters = &mut shared.boxes[self.instance].counters;
        match kind {
            Kind::Generic => counters.sent_generic += 1,
            Kind::Cmd => counters.sent_cmd += 1,
            Kind::Reply(_) => counters.sent_reply += 1,
            Kind::Ack => counters.sent_ack += 1,
        }
        shared.log.push_back(Event {
            sender: self.instance,
            kind,
            timestamp,
            len,
        });
        if shared.log.len() > LOG_LIMIT {
            shared.log.pop_front();
        }
        shared.boxes[self.instance].active = Some(Instant::now());
        log::debug!(
            "mp: {} sent {} len={len} ts={timestamp}",
            self.instance,
            kind.label()
        );
    }

    pub(crate) fn send(&self, kind: Kind, data: &[u8], timestamp: u64) -> i32 {
        let port = self.port();
        let n = match kind {
            Kind::Generic => port.send_packet(data, timestamp),
            Kind::Cmd => port.send_cmd(data, timestamp),
            Kind::Reply(aid) => port.send_reply(data, timestamp, aid),
            Kind::Ack => port.send_ack(data, timestamp),
        };
        self.sent(kind, data.len(), timestamp);
        n as i32
    }

    /// Translate a receive into the shim's convention and count it.
    fn received(&self, r: Recv, timestamp: &mut u64, host: bool) -> i32 {
        match r {
            Recv::Empty => 0,
            Recv::HostGone => -1,
            Recv::Frame { len, timestamp: ts } => {
                *timestamp = ts;
                let mut shared = self.airwaves.0.0.lock().unwrap();
                let counters = &mut shared.boxes[self.instance].counters;
                if host {
                    counters.recv_cmd += 1;
                } else {
                    counters.recv_generic += 1;
                }
                len as i32
            }
        }
    }
}

impl melonds::Host for Client {
    fn mp_begin(&self) {
        self.begin();
    }

    fn mp_end(&self) {
        self.end();
    }

    fn mp_send_packet(&self, data: &[u8], timestamp: u64) -> i32 {
        self.send(Kind::Generic, data, timestamp)
    }

    fn mp_send_cmd(&self, data: &[u8], timestamp: u64) -> i32 {
        self.send(Kind::Cmd, data, timestamp)
    }

    fn mp_send_reply(&self, data: &[u8], timestamp: u64, aid: u16) -> i32 {
        self.send(Kind::Reply(aid), data, timestamp)
    }

    fn mp_send_ack(&self, data: &[u8], timestamp: u64) -> i32 {
        self.send(Kind::Ack, data, timestamp)
    }

    fn mp_recv_packet(&self, data: &mut [u8], _now: u64, timestamp: &mut u64) -> Option<i32> {
        let r = self.port().recv_packet(data);
        Some(self.received(r, timestamp, false))
    }

    fn mp_recv_host_packet(&self, data: &mut [u8], _now: u64, timestamp: &mut u64) -> Option<i32> {
        let r = self.port().recv_host_packet(data);
        Some(self.received(r, timestamp, true))
    }

    fn mp_recv_replies(&self, data: &mut [u8], _now: u64, timestamp: u64, aidmask: u16) -> u16 {
        let mask = self.port().recv_replies(data, timestamp, aidmask);
        let mut shared = self.airwaves.0.0.lock().unwrap();
        let counters = &mut shared.boxes[self.instance].counters;
        counters.recv_reply += mask.count_ones() as u64;
        counters.last_reply_mask = mask;
        mask
    }

    fn mp_clock(&self, now: u64) {
        let mut shared = self.airwaves.0.0.lock().unwrap();
        shared.boxes[self.instance].counters.clock = now;
        shared.boxes[self.instance].active = Some(Instant::now());
    }
}
