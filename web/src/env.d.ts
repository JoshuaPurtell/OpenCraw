interface ImportMetaEnv {
  readonly OPENCRAW_WS_URL?: string
  readonly VITE_WS_URL?: string
}

interface ImportMeta {
  readonly env?: ImportMetaEnv
}
