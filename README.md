# airadb

Interactive Android wireless debugging pairing for macOS.

`airadb` wraps the ADB wireless debugging flow so you do not have to remember the pairing and connect commands. It guides you through a progressive terminal flow with QR pairing, live waits, retry options, and manual fallbacks when Android exposes a stale endpoint. Once the phone is ready, it can launch `scrcpy`.

## Install

Install the latest GitHub release:

```sh
curl -fsSL https://github.com/ovitrif/airadb/releases/latest/download/install.sh | sh
```

Pin a release or install somewhere else:

```sh
curl -fsSL https://github.com/ovitrif/airadb/releases/latest/download/install.sh | \
  AIRADB_INSTALL_TAG=v0.1.5 AIRADB_INSTALL_DIR="$HOME/.local/bin" sh
```

Or build from source:

```sh
git clone https://github.com/ovitrif/airadb.git
cd airadb
cargo build --release
```

## Usage

After installing, run:

```sh
airadb
```

The installer also sets up `aw` as a short alias for `airadb`. Remember it as
**android wifi**:

```sh
aw
```

Or from a source checkout:

```sh
cargo run
```

`airadb` expects `adb` to be installed and available on your `PATH`. `scrcpy` is optional, but needed if you want to start screen mirroring from the final menu or with `--background` / `--foreground`. The default wait time for pairing and connection discovery is 60 seconds. By default, scrcpy launches with a borderless Pixel-style window title; pass `--plain-window` to use scrcpy's regular decorated window.

On your Android phone:

1. Go to Developer options -> Wireless debugging.
2. Tap Pair device with QR code.
3. Scan the QR code shown by `airadb`.

Once ADB is connected, `airadb` shows options to start `scrcpy` in the background and close the CLI, start `scrcpy`, or close without launching anything. Use `--background` or `--foreground` to skip that final menu. If a device is already connected through ADB, `airadb` skips pairing and offers the `scrcpy` options immediately unless a launch flag was provided.

Useful options:

```sh
airadb --reset-adb
airadb --timeout 120 # wait longer than the 60-second default
airadb --background # start scrcpy in the background and close
airadb --foreground # start scrcpy in the foreground
airadb --stable # background scrcpy, ADB keepalive, reconnects, stay-awake and Wi-Fi diagnostics
airadb --watch --wifi-doctor # supervise wireless ADB and print Wi-Fi changes
airadb --plain-window --always-on-top --window-title "Pixel 10 Pro"
airadb --adb /path/to/adb --scrcpy /path/to/scrcpy
airadb install-shell # install the aw alias and zsh completions
airadb completions zsh --name aw
airadb --help
```
