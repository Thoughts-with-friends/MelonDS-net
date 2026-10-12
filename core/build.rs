//! Builds melonDS's network layer (from the `melonDS` submodule), ENet (from
//! the `enet` submodule), libslirp (vendored inside melonDS) and the C ABI
//! shim, as static archives linked into the Rust crate.
//!
//! Two arrangements matter:
//!
//! - **Namespace rename.** Every melonDS translation unit is compiled with
//!   `-DmelonDS=melonDSnet`, so the symbols this library defines
//!   (`Platform::Log`, `LocalMP`, `Wifi::GetMAC`, ...) can never collide with
//!   a full melonDS core linked into the same binary.
//! - **Clang on Windows.** melonDS is GCC/Clang code; on the MSVC target it
//!   is compiled by clang (which emits MSVC-ABI objects), found through
//!   `LLVM_ROOT`, `PATH`, `C:\Program Files\LLVM` or Visual Studio's bundled
//!   LLVM component.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().unwrap().to_path_buf();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let windows = target_os == "windows";
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");

    let melon = root.join("melonDS").join("src");
    let net = melon.join("net");
    let slirp = net.join("libslirp");
    let enet = root.join("enet");
    let shim = manifest.join("shim");

    assert!(
        melon.join("net").join("LAN.cpp").exists() && enet.join("host.c").exists(),
        "melonDS / enet sources missing: run `git submodule update --init`"
    );

    let clang = msvc.then(find_clang);
    let crt_static = std::env::var("CARGO_CFG_TARGET_FEATURE")
        .map(|f| f.split(',').any(|f| f == "crt-static"))
        .unwrap_or(false);
    let configure = |b: &mut cc::Build| {
        if let Some(bin) = &clang {
            b.archiver(bin.join("llvm-lib.exe")).flag(if crt_static {
                "-fms-runtime-lib=static"
            } else {
                "-fms-runtime-lib=dll"
            });
        }
        b.warnings(false);
    };

    // -- ENet ---------------------------------------------------------------
    let mut b = cc::Build::new();
    configure(&mut b);
    if let Some(bin) = &clang {
        b.compiler(bin.join("clang.exe"));
    }
    b.include(enet.join("include"));
    for f in [
        "callbacks.c",
        "compress.c",
        "host.c",
        "list.c",
        "packet.c",
        "peer.c",
        "protocol.c",
    ] {
        b.file(enet.join(f));
    }
    if windows {
        b.file(enet.join("win32.c"))
            .define("_WINSOCK_DEPRECATED_NO_WARNINGS", None);
    } else {
        b.file(enet.join("unix.c"));
        for d in [
            "HAS_FCNTL",
            "HAS_POLL",
            "HAS_GETADDRINFO",
            "HAS_GETNAMEINFO",
            "HAS_INET_PTON",
            "HAS_INET_NTOP",
            "HAS_MSGHDR_FLAGS",
            "HAS_SOCKLEN_T",
        ] {
            b.define(d, "1");
        }
    }
    b.compile("mdsnet_enet");

    // -- libslirp -----------------------------------------------------------
    let slirp_gen = out.join("slirp-gen");
    std::fs::create_dir_all(&slirp_gen).unwrap();
    let version = std::fs::read_to_string(slirp.join("src").join("libslirp-version.h.in"))
        .unwrap()
        .replace("@SLIRP_MAJOR_VERSION@", "4")
        .replace("@SLIRP_MINOR_VERSION@", "8")
        .replace("@SLIRP_MICRO_VERSION@", "0")
        .replace("@SLIRP_VERSION_STRING@", "\"4.8.0\"");
    std::fs::write(slirp_gen.join("libslirp-version.h"), version).unwrap();

    let mut b = cc::Build::new();
    configure(&mut b);
    if let Some(bin) = &clang {
        b.compiler(bin.join("clang.exe"));
    }
    b.include(slirp.join("glib"))
        .include(slirp.join("src"))
        .include(&slirp_gen)
        .define("LIBSLIRP_STATIC_BUILD", None)
        .define("BUILDING_LIBSLIRP", None)
        .define("G_LOG_DOMAIN", "\"Slirp\"");
    if windows {
        b.define("_WINSOCK_DEPRECATED_NO_WARNINGS", None)
            .define("_CRT_SECURE_NO_WARNINGS", None);
    } else {
        b.flag("-fvisibility=hidden");
    }
    for f in [
        "arp_table",
        "bootp",
        "cksum",
        "dhcpv6",
        "dnssearch",
        "if",
        "ip6_icmp",
        "ip6_input",
        "ip6_output",
        "ip_icmp",
        "ip_input",
        "ip_output",
        "mbuf",
        "misc",
        "ncsi",
        "ndp_table",
        "sbuf",
        "slirp",
        "socket",
        "state",
        "stream",
        "tcp_input",
        "tcp_output",
        "tcp_subr",
        "tcp_timer",
        "tftp",
        "udp6",
        "udp",
        "util",
        "version",
        "vmstate",
    ] {
        b.file(slirp.join("src").join(format!("{f}.c")));
    }
    b.file(slirp.join("glib").join("glib.c"));
    b.compile("mdsnet_slirp");

    // -- melonDS net layer + shim (C++) --------------------------------------
    let mut b = cc::Build::new();
    b.cpp(true);
    configure(&mut b);
    if let Some(bin) = &clang {
        b.compiler(bin.join("clang++.exe"));
    }
    b.std("c++17")
        .include(&shim)
        .include(&melon)
        .include(&net)
        .include(slirp.join("src"))
        .include(&slirp_gen)
        .include(enet.join("include"))
        .define("melonDS", "melonDSnet")
        .define("PacketDispatcher", "MdsNetPacketDispatcher")
        .define("LIBSLIRP_STATIC_BUILD", None);
    if windows {
        b.define("__WIN32__", None)
            .define("_CRT_SECURE_NO_WARNINGS", None)
            .define("_WINSOCK_DEPRECATED_NO_WARNINGS", None)
            .define("NOMINMAX", None);
    }
    for f in [
        "MPInterface.cpp",
        "LocalMP.cpp",
        "LAN.cpp",
        "Net.cpp",
        "Net_Slirp.cpp",
        "Net_PCap.cpp",
        "PacketDispatcher.cpp",
    ] {
        b.file(net.join(f));
    }
    b.file(melon.join("WifiAP.cpp"))
        .file(shim.join("mdsnet.cpp"))
        .file(shim.join("platform.cpp"));
    b.compile("mdsnet");

    if windows {
        for lib in ["ws2_32", "iphlpapi", "winmm"] {
            println!("cargo:rustc-link-lib=dylib={lib}");
        }
    } else if target_os == "linux" {
        println!("cargo:rustc-link-lib=dylib=dl");
    } else if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=resolv");
    }

    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=LLVM_ROOT");
}

/// The `bin` directory of an LLVM install with `clang.exe`.
fn find_clang() -> PathBuf {
    let has = |dir: &Path| dir.join("clang.exe").exists();
    if let Ok(root) = std::env::var("LLVM_ROOT") {
        return PathBuf::from(root).join("bin");
    }
    if let Some(dir) = std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .find(|d| has(d))
    {
        return dir;
    }
    let standalone = PathBuf::from(r"C:\Program Files\LLVM\bin");
    if has(&standalone) {
        return standalone;
    }
    let vswhere =
        PathBuf::from(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    if let Ok(o) = std::process::Command::new(vswhere)
        .args(["-all", "-products", "*", "-property", "installationPath"])
        .output()
    {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let dir = PathBuf::from(line.trim()).join(r"VC\Tools\Llvm\x64\bin");
            if has(&dir) {
                return dir;
            }
        }
    }
    panic!("clang not found: install LLVM (or VS's C++ Clang component), or set LLVM_ROOT");
}
