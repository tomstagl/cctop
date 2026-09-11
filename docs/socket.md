# Session messaging socket

Every interactive Claude Code session listens on a Unix socket (registry
field `messagingSocketPath`, e.g. `/tmp/cc-socks/<pid>.sock`) and publishes an
auth key next to its registry file: `~/.claude/sessions/<pid>.<hash>.key`
containing `{"peerToken": "…", "procStart": "…", "pidDomain": "…"}`.

## Wire format (as documented inside Claude Code 2.1.269)

Newline-delimited JSON. The first line authenticates, the next carries a
message:

```
{"type":"auth","token":"<peerToken>"}
{"type":"user","message":{"role":"user","content":"hello"}}
```

Claude Code's own log text describes exactly this: *"Inject messages (auth
line REQUIRED here): `{ echo '{"type":"auth","token":"'"$CLAUDE_CODE_MESSAGING_TOKEN"'"}'; echo '{"type":"user","message":{"role":"user","content":"hello"}}'; } | socat - UNIX-CONNECT:<path>`"*.
Replies seen on the channel are frames such as `{"type":"connected", …}`,
`{"type":"hb_ack"}` and `{"type":"error","reason":…}`.

## What this means for "ask from cctop"

The socket **injects a user message**; there is no "draft" slot. So `a` in
cctop offers two deliveries:

- **Enter — copy to clipboard** (default). The prompt lands in the clipboard;
  paste and submit it in Claude Code. This is the only true draft.
- **S — send over the socket**. The prompt arrives as an inbound peer message;
  the session handles it under its own peer-approval settings
  (`peer_inbound_approval`), which may prompt before acting on it.

If the socket path is missing or unreachable, cctop shows `no messaging
socket` and only the clipboard path is available.

Observed 2026-09-11: a raw `{"type":"ping"}` line gets no reply; the
auth-then-user sequence is accepted. cctop's tests exercise the framing
against a scratch Unix socket server.
