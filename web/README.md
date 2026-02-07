# OpenCraw WebChat (Bun)

This is a small Bun + React client that connects to the OpenCraw WebChat WebSocket.

## Run

1. Start the Rust server (from the repo root):

```bash
cargo run -p os-app -- serve
```

2. Start the web client:

```bash
cd web
bun install
bun run dev
```

By default the client connects to `ws://localhost:3000/ws`.

To override at build time:

```bash
OPENCRAW_WS_URL=ws://localhost:3000/ws bun run build
```
