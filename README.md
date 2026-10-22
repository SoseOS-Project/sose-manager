# SoseOS manager

The VM manager side of SoseOS, a privacy focused phone OS I'm building. It's Qubes-like: everything runs in separate Xen VMs and a tiny dom0 manages them.

Early and changing a lot, but it works. Runs on ARM64 under Xen (tested in QEMU).

## What's in here

- `xenstore/` - small Rust client for xenstored's socket protocol, written from the Xen docs. read, directory and write work.
- `svmd/` - the VM manager daemon. Listens on `/run/svmd.sock` and starts/stops VMs.
- `svm-client/` - the bits svmd and its tools share (socket path, request helper).
- `utils/` - terminal helpers: colors (only on a real terminal, respects NO_COLOR) and aligned tables.
- `tools/svm/` - command line client for svmd, `svm start <name>` etc. (work in progress)

## svmd

Send it one command per line:

```
list           # VMs it knows about (/etc/sose/vms/<name>.cfg)
start <name>   # xl create /etc/sose/vms/<name>.cfg
stop <name>    # xl destroy <name>
```

Every reply ends with `ok` or `err <reason>`. For now it just calls `xl`, using libxl directly is planned.

Quick test inside dom0:

```
socat - UNIX-CONNECT:/run/svmd.sock
```

## Building

Outside crates are vendored in `vendor/` so everything builds offline.

```
cargo build --release --target aarch64-unknown-linux-gnu
cargo test
```

In SoseOS these get built into dom0 with Buildroot.

## License

GPL-2.0-only, see `LICENSE`.
