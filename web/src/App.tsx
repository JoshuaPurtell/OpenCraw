import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'

type InboundHello = { type: 'hello'; sender_id: string }
type InboundMessage = { type: 'message'; content: string }
type InboundPayload = InboundHello | InboundMessage | Record<string, unknown>
type ConnectionStatus = 'disconnected' | 'connecting' | 'connected'

type ChatItem = {
  id: string
  at: number
  role: 'you' | 'assistant' | 'system'
  text: string
}

type AutomationStatusPayload = {
  status?: string
  automation?: {
    enabled?: boolean
    heartbeat_ticks?: number
    scheduler_ticks?: number
    scheduler_runs?: number
    scheduler_failures?: number
    job_count?: number
    enabled_job_count?: number
  }
}

type SkillsPayload = {
  status?: string
  skills?: Array<{ decision?: 'approve' | 'warn' | 'block' }>
}

type SessionsPayload = {
  sessions?: Array<{
    id: string
    channel_id: string
    sender_id: string
    messages: number
    model_override?: string | null
  }>
}

type ChannelsPayload = {
  channels?: string[]
}

const statusStyles: Record<
  ConnectionStatus,
  { label: string; tone: string }
> = {
  connected: {
    label: 'connected',
    tone:
      'border-[hsl(var(--accent-support)/0.7)] bg-[hsl(var(--accent-support)/0.34)] text-[hsl(var(--ink))] status-active',
  },
  connecting: {
    label: 'connecting',
    tone:
      'border-[hsl(var(--accent-alt)/0.7)] bg-[hsl(var(--accent-alt)/0.3)] text-[hsl(var(--ink))]',
  },
  disconnected: {
    label: 'disconnected',
    tone:
      'border-[hsl(var(--line))] bg-[hsl(var(--surface-strong)/0.72)] text-[hsl(var(--ink-muted))]',
  },
}

const messageStyles: Record<
  ChatItem['role'],
  { row: string; bubble: string; label: string }
> = {
  you: {
    row: 'justify-end',
    bubble:
      'border-[hsl(var(--accent)/0.6)] bg-[hsl(var(--accent)/0.24)] text-[hsl(var(--ink))]',
    label: 'you',
  },
  assistant: {
    row: 'justify-start',
    bubble:
      'border-[hsl(var(--accent-alt)/0.6)] bg-[hsl(var(--accent-alt)/0.22)] text-[hsl(var(--ink))]',
    label: 'assistant',
  },
  system: {
    row: 'justify-center',
    bubble:
      'max-w-[30rem] border-[hsl(var(--line))] bg-[hsl(var(--surface-strong)/0.78)] text-[hsl(var(--ink-muted))]',
    label: 'system',
  },
}

function StatusChip({ status }: { status: ConnectionStatus }) {
  const state = statusStyles[status]
  return <span className={`chip ${state.tone}`}>{state.label}</span>
}

function MessageRow({ item }: { item: ChatItem }) {
  const style = messageStyles[item.role]
  const timeLabel = new Date(item.at).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  })

  return (
    <div className={`flex ${style.row} animate-rise`}>
      <article
        className={`w-full max-w-[44rem] rounded-2xl border px-4 py-3 shadow-[0_18px_30px_-20px_hsl(var(--ink)/0.48)] ${style.bubble}`}
      >
        <div className="mb-1 flex items-center justify-between text-[0.63rem] font-semibold uppercase tracking-[0.14em] text-[hsl(var(--ink-muted))]">
          <span>{style.label}</span>
          <span>{timeLabel}</span>
        </div>
        <p className="m-0 whitespace-pre-wrap break-words text-[0.95rem] leading-relaxed">
          {item.text}
        </p>
      </article>
    </div>
  )
}

function App() {
  const wsUrl = useMemo(() => {
    const fromEnv = import.meta.env?.OPENCRAW_WS_URL ?? import.meta.env?.VITE_WS_URL
    return fromEnv?.trim() ? fromEnv : 'ws://localhost:3000/ws'
  }, [])

  const wsRef = useRef<WebSocket | null>(null)
  const [status, setStatus] = useState<ConnectionStatus>('connecting')
  const [items, setItems] = useState<ChatItem[]>([])
  const [draft, setDraft] = useState('')
  const trimmedDraft = draft.trim()

  const append = useCallback((role: ChatItem['role'], text: string) => {
    setItems((prev) => [
      ...prev,
      { id: crypto.randomUUID(), at: Date.now(), role, text },
    ])
  }, [])

  const openSocket = useCallback(
    (force = false) => {
      const cur = wsRef.current
      if (
        cur &&
        (cur.readyState === WebSocket.OPEN ||
          cur.readyState === WebSocket.CONNECTING)
      ) {
        if (!force) return
        wsRef.current = null
        cur.close()
      }

      const ws = new WebSocket(wsUrl)
      wsRef.current = ws

      ws.addEventListener('open', () => {
        if (wsRef.current !== ws) return
        setStatus('connected')
        append('system', 'Connected')
      })

      ws.addEventListener('message', (evt) => {
        if (wsRef.current !== ws || typeof evt.data !== 'string') return

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
        if (wsRef.current !== ws) return
        wsRef.current = null
        setStatus('disconnected')
        append('system', 'Disconnected')
      })

      ws.addEventListener('error', () => {
        if (wsRef.current !== ws) return
        append('system', 'WebSocket error')
      })
    },
    [append, wsUrl],
  )

  useEffect(() => {
    openSocket()
    return () => {
      const ws = wsRef.current
      wsRef.current = null
      ws?.close()
    }
  }, [openSocket])

  function reconnect() {
    setStatus('connecting')
    append('system', `Connecting to ${wsUrl}`)
    openSocket(true)
  }

  function sendMessage() {
    if (!trimmedDraft) {
      return
    }

    append('you', trimmedDraft)
    setDraft('')

    const ws = wsRef.current
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      append('system', 'Not connected (message not sent)')
      return
    }

    ws.send(JSON.stringify({ type: 'message', content: trimmedDraft }))
  }

  return (
    <div className="flex h-screen flex-col">
      <header className="flex items-center gap-3 border-b border-[hsl(var(--line)/0.8)] bg-[hsl(var(--surface)/0.97)] px-4 py-2 sm:px-6">
        <div className="grid h-8 w-8 shrink-0 place-items-center rounded-md border border-[hsl(var(--accent)/0.7)] bg-[hsl(var(--accent)/0.22)] text-[0.6rem] font-black tracking-[0.2em] text-[hsl(var(--ink))]">
          OC
        </div>
        <h1 className="m-0 text-base font-bold tracking-tight text-[hsl(var(--ink))]">
          OpenCraw Console
        </h1>
        <StatusChip status={status} />
        <div className="flex-1" />
        <button
          type="button"
          onClick={reconnect}
          className="inline-flex h-8 items-center justify-center rounded-lg border border-[hsl(var(--accent-alt)/0.7)] bg-[linear-gradient(180deg,hsl(var(--accent-alt)),hsl(22_42%_38%))] px-3 text-xs font-semibold text-white transition-all duration-200 hover:brightness-110"
        >
          Reconnect
        </button>
      </header>

      <section
        className="chat-scroll flex-1 space-y-3 overflow-y-auto bg-[hsl(var(--canvas-deep))] px-4 py-4 sm:px-6 sm:py-6"
        role="log"
        aria-label="Chat log"
      >
        {items.length === 0 ? (
          <div className="mx-auto max-w-md rounded-2xl border border-dashed border-[hsl(var(--accent)/0.5)] bg-[hsl(var(--surface)/0.97)] px-5 py-6 text-center text-sm text-[hsl(var(--ink-muted))]">
            Waiting for events. Once the websocket responds, messages will appear here.
          </div>
        ) : (
          items.map((item) => <MessageRow key={item.id} item={item} />)
        )}
      </section>

      <form
        className="border-t border-[hsl(var(--line)/0.8)] bg-[hsl(var(--surface)/0.97)] p-4 sm:p-5"
        onSubmit={(event) => {
          event.preventDefault()
          sendMessage()
        }}
      >
        <label htmlFor="chat-message" className="sr-only">
          Message
        </label>
        <div className="flex flex-col gap-3 sm:flex-row">
          <input
            id="chat-message"
            className="h-12 flex-1 rounded-xl border border-[hsl(var(--line)/0.98)] bg-[hsl(var(--surface))] px-4 text-[0.96rem] text-[hsl(var(--ink))] shadow-inner shadow-[hsl(var(--line)/0.3)] transition-all duration-200 placeholder:text-[hsl(var(--ink-muted))] focus:border-[hsl(var(--accent)/0.64)] focus:outline-none focus:ring-4 focus:ring-[hsl(var(--accent)/0.18)]"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Send a message to the assistant"
            autoComplete="off"
          />
          <button
            type="submit"
            disabled={!trimmedDraft}
            className="inline-flex h-12 items-center justify-center rounded-xl border border-[hsl(var(--accent)/0.7)] bg-[linear-gradient(180deg,hsl(182_48%_34%),hsl(var(--accent)))] px-6 text-sm font-semibold text-white shadow-[0_16px_24px_-16px_hsl(var(--accent)/0.72)] transition-all duration-200 hover:-translate-y-0.5 hover:brightness-110 disabled:cursor-not-allowed disabled:border-[hsl(var(--line))] disabled:bg-[hsl(var(--surface-strong))] disabled:text-[hsl(var(--ink-muted))] disabled:shadow-none disabled:hover:translate-y-0"
          >
            Send Message
          </button>
        </div>
      </form>
    </div>
  )
}

export default App
