# T-008 Linux validation environment

Use only the isolated QEMU guest. Do not deploy these fixtures to a user server.
Downloads used for this run are retained under ignored `.qa/t008/linux`:

- QEMU Windows build `qemu-w64-setup-20260811.exe`, downloaded from
  https://qemu.weilnetz.de/w64/ and extracted with 7-Zip (no system installation).
  SHA-512 `5bcf9eed634e8575a37b74f445af41a2fe4106da512d0c30c368301d4c105037fdfab40a5287367a28a957624cddebbc8c07e16c88ab6634f554cdf3d16bf543`.
- Debian 13 genericcloud amd64 image from https://cloud.debian.org/images/cloud/trixie/latest/;
  exact published SHA512SUMS and matching image are retained with the VM.
- Guest runtime: 2 vCPU, 2 GiB RAM, 12 GiB qcow2 overlay over immutable base;
  SSH forwarded to loopback 22280, no host directory sharing. QEMU licenses are
  retained in its extracted distribution. No Windows service/feature is installed.

`seed.py` requires pycdlib and paramiko in an isolated Python target. It creates a
private test key and NoCloud ISO in the ignored directory. The guest has ordinary
tester/reader accounts for product tests; root is used only for fixture setup.
Stop the specific recorded VM PID after validation. Never delete mounted guest
trees; unmount fixture mounts before deleting fixture files.

`cleanup.py --config <private-config.json> --output <evidence.json>` refuses any
endpoint except the isolated loopback port. It unmounts nested and stacked fixture
mounts first, stops guest Samba, requires the test accounts to have no processes,
then requests guest poweroff. It retains all files and VM disks. Verify the exact
recorded QEMU PID and forwarded port are gone from the host after shutdown.
