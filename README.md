# MelonDS-net

Nintendo DS networking for Rust emulators, powered by melonDS's C++ network layer through a small C ABI.

`melonds_net` provides the networking layer for emulators that implement the DS Wi-Fi hardware. Your emulator supplies transmitted Wi-Fi frames and delivers received frames to its emulated hardware; this library handles networking between consoles and the host network.

## Features

- **Local wireless:** Connect multiple emulated consoles in one process.
- **LAN multiplayer:** Connect consoles across processes or machines using ENet.
- **Internet connectivity:** Connect through an emulated access point and host-side networking.
- **Emulated access point:** `melonAP` on channel 6, with 802.11-to-Ethernet bridging for Nintendo Wi-Fi Connection (WFC)-style traffic.
- **Reusable Rust API:** Use the high-level `Station` interface or access individual networking components.

### Components

| Rust API  | melonDS implementation             | Purpose                                     |
| --------- | ---------------------------------- | ------------------------------------------- |
| `mp`      | `net/MPInterface`, `net/LocalMP`   | Local wireless multiplayer                  |
| `lan`     | `net/LAN` + ENet                   | LAN sessions, hosting, joining, discovery   |
| `net`     | `net/Net`, `Net_Slirp`, `Net_PCap` | Host networking via NAT or adapter bridging |
| `ap`      | `WifiAP`                           | Emulated access point                       |
| `Station` | `Wifi.cpp` TX/RX routing           | Unified frame transmission and reception    |

Frames use melonDS's format: a 12-byte TX/RX header followed by the 802.11 frame. Timestamps use the emulated console's Wi-Fi microsecond counter.

## Project layout

```text
melonDS/            melonDS git submodule
enet/               ENet git submodule
core/               melonds_net Rust crate
  shim/mdsnet.h     C ABI
  shim/*.cpp        C ABI implementation and platform shim
  build.rs          Native dependency build
  src/              Rust API
  tests/            Integration tests
  examples/         Minimal usage example
gui/melon_egui/     Test emulator frontend
```

The build compiles the melonDS network sources with the namespace renamed to `melonDSnet`. This allows `melonds_net` to coexist with a full melonDS core in the same binary without symbol clashes.

## Build and installation

Initialize the native dependencies:

```sh
git submodule update --init
```

Run the library tests:

```sh
cargo test -p melonds_net
```

### Windows requirements

The build requires Clang for C++ compilation. It searches for Clang in the following locations:

- `LLVM_ROOT`
- `PATH`
- `C:\Program Files\LLVM`
- Visual Studio's **C++ Clang tools** component, discovered through `vswhere`

### Use as a dependency

Add `melonds_net` to your `Cargo.toml`:

```toml
[dependencies]
melonds_net = { git = "https://github.com/Thoughts-with-friends/MelonDS-net" }
```

For a local checkout:

```toml
[dependencies]
melonds_net = { path = "../MelonDS-net/core" }
```

When using a local checkout, initialize its git submodules before building. When using a Git dependency, Cargo fetches the dependency's repository, including its configured submodules.

## Quick start

### 1. Initialize the library

Initialize logging and select the multiplayer backend once during startup:

```rust
melonds_net::init_logging();

melonds_net::mp::set_backend(
    melonds_net::MpBackend::Local,
);
```

Choose the backend before any console starts transmitting. The backend is process-wide; do not switch it while a console is inside an MP operation.

### 2. Choose a networking mode

#### Local wireless

Use `MpBackend::Local` to connect consoles within the same process.

#### LAN multiplayer

Set the LAN backend and create or join a session:

```rust
use melonds_net::{lan, mp};

mp::set_backend(mp::MpBackend::Lan);

// Host
lan::host("Alice", 4)?;

// Or join an existing host
// lan::join("Bob", "192.168.0.10")?;
```

The host binds UDP port `7064` and answers discovery requests on UDP port `7063`.

The host must call `mp::process()` regularly. If a client joins before the host's emulation loop starts, `lan::join` may time out; retry while the host is starting.

To discover sessions on the local network:

```rust
lan::start_discovery()?;

// Keep calling mp::process() while discovery runs.
for session in lan::discovered() {
    println!(
        "{} {} ({}/{})",
        session.address,
        session.session_name,
        session.num_players,
        session.max_players,
    );
}

lan::end_discovery();
```

Use `lan::players()` to inspect session members and `lan::leave()` when shutting down.

#### Internet connectivity

Create a host-network link using the built-in user-mode NAT:

```rust
use std::sync::Arc;
use melonds_net::net::{Driver, Net};

let link = Arc::new(
    Net::with_driver(&Driver::Slirp)?
);
```

The default Slirp configuration provides the emulated DS with:

| Setting  | Address      |
| -------- | ------------ |
| IP lease | `10.64.0.16` |
| Gateway  | `10.64.0.1`  |
| DNS      | `10.64.0.2`  |

For adapter bridging, use the PC's network adapter through `Driver::Pcap`. This requires libpcap or Npcap:

```rust
let adapters = melonds_net::net::pcap_adapters();

// Construct Driver::Pcap with the selected adapter's device name.
```

One `Net` instance can be shared by all consoles in a process.

### 3. Create a `Station` for each console

```rust
use melonds_net::Station;
use std::sync::Arc;

let firmware_mac = [
    0x00, 0x09, 0xBF, 0x12, 0x34, 0x56,
];

let mut station = Station::new(
    0,
    firmware_mac,
    Some(Arc::clone(&link)),
);
```

- The first argument is the console's instance ID (`0..16`).
- The MAC address should come from the emulated console's firmware.
- Pass `None` instead of a network link when no access point or Internet connection is needed.

Creating a `Station` begins its MP session. Dropping it ends the session.

### 4. Integrate with your Wi-Fi emulation

Call the appropriate methods from your emulated Wi-Fi MAC:

| Hardware event                     | Library call                                                   |
| ---------------------------------- | -------------------------------------------------------------- |
| Transmit a LOC1–3 packet or beacon | `station.transmit(FrameKind::Packet, &frame, us_count)`        |
| Transmit an MP command             | `station.transmit(FrameKind::Cmd, &frame, us_count)`           |
| Send an MP reply                   | `station.transmit(FrameKind::Reply { aid }, &frame, us_count)` |
| Send an MP acknowledgement         | `station.transmit(FrameKind::Ack, &frame, us_count)`           |
| Receive incoming frames            | `station.receive(&mut buffer)`                                 |
| Advance emulated Wi-Fi time        | `station.tick(us_count)`                                       |

For multiplayer-specific receive operations:

- The host collects client replies through `station.port().recv_replies(...)`.
- A client receives the host's command or acknowledgement through `station.port().recv_host_packet(...)`.

See [Integrating with a Wi-Fi MAC](#integrating-with-a-wi-fi-mac) for buffer sizes, return values, frame layout, and timing requirements.

Once per emulated video frame, call:

```rust
melonds_net::mp::process();
```

This is required for LAN and harmless for Local mode.

### 5. Shut down

Drop each `Station`. For LAN sessions, call `lan::leave()`. Drop the shared `Net` after the last station using it has been dropped.

## Integrating with a Wi-Fi MAC

### Frame format

Every transmitted or received frame uses:

```text
12-byte melonDS header | 802.11 frame, including the 4-byte FCS
```

The header fields have different meanings for TX and RX.

#### Transmit (TX)

- Bytes `0..12` contain the TX header from Wi-Fi RAM, as described in GBATEK's DS Wi-Fi Transmit Buffers.
- Byte `9` must contain the current Wi-Fi channel, matching melonDS's `Wifi.cpp` behavior.
- The emulated timestamp must be the console's `W_US_COUNT` value at send time.

The access point ignores frames whose channel byte does not match its channel (`6`).

#### Receive (RX)

- Byte `8`: rate (`10` = 1 Mbit/s, `20` = 2 Mbit/s).
- Byte `9`: channel.
- Bytes `10..12`: frame length.
- The emulator must copy the RX header and frame body into its emulated Wi-Fi RX buffer.

Use a buffer of `melonds_net::RX_BUFFER_SIZE` bytes for `Station::receive`.

### Multiplayer receive buffers

On the host, `recv_replies` collects replies indexed by AID:

```rust
let mask = station.port().recv_replies(
    &mut buffer,
    cmd_us_count,
    aidmask,
);
```

Reply data for AID `n` is stored at:

```rust
buffer[(n - 1) * 1024..]
```

Size the buffer to `mp::REPLY_BUFFER_SIZE`.

On a client, use:

```rust
station.port().recv_host_packet(&mut buffer)
```

The result is `Recv::Frame`, `Recv::Empty`, or `Recv::HostGone`.

Timestamps are important: stale multiplayer replies are filtered against the command timestamp. Always use emulated Wi-Fi time, not wall-clock time.

### Timing and threading

`recv_host_packet` and `recv_replies` block for up to `mp::recv_timeout()` milliseconds (25 ms by default).

With `MpBackend::Local`, two consoles in the same process must run on separate threads if both can block while waiting for each other's replies. On a single thread, one console may wait for a reply that the other console cannot send.

Use `mp::set_recv_timeout` to change the receive timeout.

| Type      | Threading                             |
| --------- | ------------------------------------- |
| `Net`     | `Send + Sync`                         |
| `Station` | `Send`; use from one thread at a time |
| `WifiAp`  | `Send`; use from one thread at a time |
| `MpPort`  | `Send`; use from one thread at a time |

### Lower-level APIs

`Station` is a convenience wrapper. Use the individual components when your emulator needs custom routing:

- `mp::MpHandle`: explicit `begin()` / `end()` lifecycle; useful when the emulator already manages Wi-Fi power state.
- `mp::MpPort`: an MP handle that begins on creation and ends on drop.
- `ap::WifiAp`: the access point interface (`send`, `recv`, `ms_timer`, `reset`).
- `net::Net`: raw Ethernet frames per instance (`send`, `recv`).

The `melon_egui` frontend uses `MpHandle` to connect melonDS's `Platform::MP_*` hooks to this library.

## Examples and API reference

Read the API documentation in this order:

1. Generate the crate documentation:

   ```sh
   cargo doc -p melonds_net --open
   ```

2. Start with [`core/src/lib.rs`](core/src/lib.rs) for `Station` and `FrameKind`.
3. Read the module that matches your use case:
   - [Local and LAN multiplayer](core/src/mp.rs) and [LAN sessions](core/src/lan.rs)
   - [Host networking](core/src/net.rs) and [the access point](core/src/ap.rs)
4. Read [`core/shim/mdsnet.h`](core/shim/mdsnet.h) if calling the C ABI directly through `melonds_net::ffi` or from another language.

### Tests and working examples

| File                                                                 | What it tests                                        |
| -------------------------------------------------------------------- | ---------------------------------------------------- |
| [`core/tests/local_mp.rs`](core/tests/local_mp.rs)                   | Local MP command → reply → ACK                       |
| [`core/tests/lan.rs`](core/tests/lan.rs)                             | Host and client in separate processes                |
| [`core/tests/wfc_ap.rs`](core/tests/wfc_ap.rs)                       | AP authentication/association and DHCP through Slirp |
| [`core/examples/minimal.rs`](core/examples/minimal.rs)               | Minimal integration example                          |
| [`gui/melon_egui/src/mp/client.rs`](gui/melon_egui/src/mp/client.rs) | Emulator frontend integration                        |

## Test frontend

`melon_egui` provides a frontend for manual testing:

```sh
# Local multiplayer
cargo run -p melon_egui -- <rom.nds>

# LAN host
cargo run -p melon_egui -- --lan-host <rom.nds>

# Join a LAN host
cargo run -p melon_egui -- --lan-join 192.168.0.10 <rom.nds>
```

For end-to-end testing, start two instances, enter a wireless mode in both games, and inspect the counters in **System → Multiplayer → Wireless status**.

Passing the library tests or launching a game does not by itself prove that a real game has completed a wireless data exchange.

## Troubleshooting

| Symptom                                           | Possible cause / fix                                                                                                          |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `lan::host` returns `Failed`                      | UDP port `7064` is already in use.                                                                                            |
| `lan::join` returns `Failed`                      | The host is not calling `mp::process()` yet, or a firewall blocks UDP `7064`. Retry after the host starts.                    |
| `LanError::NotLan`                                | Call `mp::set_backend(MpBackend::Lan)` before LAN operations.                                                                 |
| The AP does not answer authentication/association | Check that TX header byte `9` is channel `6` and the frame BSSID matches `ap::mac()`.                                         |
| No DHCP reply                                     | Ensure the console is associated and its `Station` was created with a `Net` link. Data frames are dropped before association. |
| A client waits about 25 ms per frame              | Two Local-mode consoles may be running on the same thread. See [Timing and threading](#timing-and-threading).                 |

## License

GPL-3.0-or-later. melonDS's license governs the combined work.