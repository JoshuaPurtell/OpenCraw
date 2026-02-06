# OpenCraw WebChat (Vite)

This is a small Vite + React client that connects to the OpenCraw WebChat WebSocket.

## Run

1. Start the Rust server (from the repo root):

```bash
cargo run -p os-app -- serve
```

2. Start the web client:

```bash
cd web
npm install
npm run dev
```

By default the client connects to `ws://localhost:3000/ws`.

To override:

```bash
VITE_WS_URL=ws://localhost:3000/ws npm run dev
```

