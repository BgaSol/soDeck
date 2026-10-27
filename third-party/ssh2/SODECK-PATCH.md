# ssh2 0.9.6 byte path adaptation

Source: crates.io `ssh2` 0.9.6 (MIT/Apache-2.0, licenses retained).
The original crate checksum is retained in `.cargo-checksum.json` and Cargo.lock's baseline.

SoDeck adds read-only byte path equivalents in `src/sftp.rs`: open_mode_bytes,
stat_bytes, lstat_bytes, readlink_bytes, realpath_bytes and readdir_bytes.
These avoid Windows path separator rewriting and non-UTF-8 filename decoding.
Original public APIs, SSH transport and cryptography are unchanged. Byte arguments
reject embedded NUL through CString. Link responses are bounded to 16 KiB and
directory entry names to 4 KiB. EOF and EAGAIN retain upstream semantics.

The core owns nonblocking handles through successful explicit close; dropping a
live upstream SFTP handle can switch the shared Session to blocking mode and must
not be used as the normal cancellation mechanism.

`Sftp::shutdown` also restores its owned native pointer when libssh2 returns
EAGAIN, so the owner can retry nonblocking shutdown. Losing the pointer on that
path would leak a channel and prevent reliable cancellation. Real OpenSSH
tests cover directory/exec cancellation while a PTY continues, and native
workspace-close tests verify per-connection SFTP/exec counts reach zero.
