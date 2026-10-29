# SoDeck local libssh2 adapter

Upstream: crates.io libssh2-sys 0.3.3, package SHA256
`0f5eb74291e8691cab524a01274a1b1e7742b1a94f29d8b101d8aadc8372c1cd`.
Bundled header version: 1.11.1_DEV. Original MIT/Apache and libssh2 COPYING
licenses are retained. The upstream package is preserved except for these changes:

- Build only the bundled static library, no vcpkg/pkg-config or git invocation.
- Numeric-only observer, separate from session.abstract. Count successful session
  socket I/O and successful channel delivery, including internal SFTP calls.
  SSH-agent FD I/O is excluded. Channel creation retains its origin across EAGAIN.
- Dedicated unknown-channel-request send/receive state machine. FAILURE is a
  valid response, timeout/cancel retires the channel. No authentication/crypto or
  business protocol changes. The caller must finish partial sends before close.
- Final-owner local session disposal disables socket I/O before the upstream
  destructor releases pending channels, so EAGAIN cannot outlive the observer.
  Normal channel/session operations retain upstream behavior.

Source additions: src/sodeck.c, public/local declarations, observer hooks in
channel.c and the session socket macros. Rust ABI declarations live in lib.rs.
Validation/limitations: T-019 E1/E2 and MCODE-02/04; see task results. This is
local source, not a claim of an upstream release or independently validated API.
