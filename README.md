# vbus-sync

Downloads recorded data from a RESOL datalogging device and converts it to CSV.


## Summary

This application downloads all recorded data files from a RESOL data logging device into a
directory named like the host name itself. If the file exists locally, the download is skipped.
After that the binary `<DATECODE>.vbus` file is converted into a `<DATECODE>.csv` for easier
handling.


## Building it

A Rust toolchain is required to build this application.
### Install Rust
* On Unix, run `curl https://sh.rustup.rs -sSf | sh` in your shell. This downloads and runs `rustup-init.sh`, which in turn downloads and runs the correct version of the `rustup-init` executable for your platform.
* On Windows, download and run `rustup-init.exe` from https://www.rust-lang.org/tools/install.

### Build the Tool

On UNIX run:
```
$ git clone .../vbus-sync
$ cd vbus-sync
$ cargo build
```
On Windows run:
```
> git clone .../vbus-sync
> cd vbus-sync
> cargo build
```

## Running it

On UNIX run:
```
$ cd .../vbus-sync
$ # with debug output to see what happens:
$ RUST_LOG=debug target/debug/vbus-sync <HOST...>
$ # without debug output:
$ target/debug/vbus-sync <HOST...>
```
On Windows run: 
```
> cd .../vbus-sync
If you want to see what happens: > set RUST_LOG=debug 
> "target/debug/vbus-sync" <HOST...>
```

## Arguments
For the Argument `<Host...>` you can either give the public IP-address (123.456.78.9) or the webinterface (d123456789.vbus.io) of your datalogging devices.    