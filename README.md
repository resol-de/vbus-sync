# vbus-sync

Downloads recorded data from a RESOL datalogging device and converts it to CSV.


## Summary

This application downloads all recorded data files from a RESOL data logging device into a
directory named like the host name itself. If the file exists locally, the download is skipped.
After that the binary `<DATECODE>.vbus` file is converted into a `<DATECODE>.csv` for easier
handling.


## Building it

A Rust toolchain is required to build this application.

```
$ git clone .../vbus-sync
$ cd vbus-sync
$ cargo build
$ ls -l target/debug/vbus-sync
```


## Running it

```
$ cd .../vbus-sync
$ # target/debug/vbus-sync <HOST...>
$ target/debug/vbus-sync 192.168.180.52
$ ls -l 192.168.180.52
```
