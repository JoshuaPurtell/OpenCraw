import { useEffect, useMemo, useRef, useState } from 'react'
import './App.css'

type InboundHello = { type: 'hello'; sender_id: string }
type InboundMessage = { type: 'message'; content: string }
type InboundPayload = InboundHello | InboundMessage | Record<string, unknown>

type ChatItem = {
  id: string
  at: number
  role: 'you' | 'assistant' | 'system'
  text: string
}

function App() {
  const wsUrl = useMemo(() => {
    const fromEnv = import.meta.env.VITE_WS_URL as string | undefined
    return fromEnv?.trim() ? fromEnv : 'ws://localhost:3000/ws'
  }, [])

  const wsRef = useRef<WebSocket | null>(null)
  const [status, setStatus] = useState<'disconnected' | 'connecting' | 'connected'>(
    'disconnected',
  )
  const [senderId, setSenderId] = useState<string | null>(null)
  const [items, setItems] = useState<ChatItem[]>([
    {
      id: crypto.randomUUID(),
      at: Date.now(),
      role: 'system',
      text: `Connecting to ${wsUrl}`,
    },
  ])
  const [draft, setDraft] = useState('')

  function append(role: ChatItem['role'], text: string) {
    setItems((prev) => [
      ...prev,
      { id: crypto.randomUUID(), at: Date.now(), role, text },
    ])
  }

  function connect() {
    const cur = wsRef.current
    if (
      cur &&
      (cur.readyState === WebSocket.OPEN ||
        cur.readyState === WebSocket.CONNECTING)
    ) {
      return
    }

    setStatus('connecting')
    setSenderId(null)
    append('system', `Connecting to ${wsUrl}`)

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.addEventListener('open', () => {
      setStatus('connected')
      append('system', 'Connected')
    })

    ws.addEventListener('message', (evt) => {
      if (typeof evt.data !== 'string') return

      let parsed: InboundPayload
      try {
        parsed = JSON.parse(evt.data) as InboundPayload
      } catch {
        append('system', `Non-JSON message: ${evt.data}`)
        return
      }

      const t = (parsed as { type?: unknown }).type
      if (t === 'hello') {
        const hello = parsed as InboundHello
        setSenderId(hello.sender_id)
        append('system', `Server assigned sender_id: ${hello.sender_id}`)
        return
      }

      if (t === 'message') {
        const msg = parsed as InboundMessage
        append('assistant', msg.content ?? '')
        return
      }

      append('system', `Unhandled payload: ${evt.data}`)
    })

    ws.addEventListener('close', () => {
      setStatus('disconnected')
      append('system', 'Disconnected')
    })

    ws.addEventListener('error', () => {
      append('system', 'WebSocket error')
    })
  }

  useEffect(() => {
    connect()
    return () => {
      wsRef.current?.close()
      wsRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wsUrl])

  function sendMessage() {
    const text = draft.trim()
    if (!text) return

    append('you', text)
    setDraft('')

    const ws = wsRef.current
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      append('system', 'Not connected (message not sent)')
      return
    }

    ws.send(JSON.stringify({ type: 'message', content: text }))
  }

  return (
    <div className="layout">
      <header className="topbar">
        <div className="brand">
          <div className="brand__mark" aria-hidden="true">
            OC
          </div>
          <div className="brand__text">
            <div className="brand__title">OpenCraw</div>
            <div className="brand__meta">
              <span className={`pill pill--${status}`}>{status}</span>
              <span className="muted">
                {senderId ? `sender_id ${senderId}` : 'awaiting hello'}
              </span>
            </div>
          </div>
        </div>

        <div className="topbar__right">
          <div className="muted mono">{wsUrl}</div>
          <button type="button" className="btn" onClick={connect}>
            Reconnect
          </button>
        </div>
      </header>

      <main className="panel">
        <div className="log" role="log" aria-label="Chat log">
          {items.map((it) => (
            <div key={it.id} className={`row row--${it.role}`}>
              <div className="bubble">
                <div className="bubble__role">{it.role}</div>
                <div className="bubble__text">{it.text}</div>
              </div>
            </div>
          ))}
        </div>

        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault()
            sendMessage()
          }}
        >
          <input
            className="input"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Send a message…"
            autoComplete="off"
          />
          <button type="submit" className="btn btn--primary">
            Send
          </button>
        </form>
      </main>
    </div>
  )
}

export default App

