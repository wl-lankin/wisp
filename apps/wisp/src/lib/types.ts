export type Os = 'Windows' | 'MacOs' | 'Linux'
export type DeliveryMode = 'Catch' | 'AutoSync'
export type ItemKind = 'Text' | 'Html' | 'Image' | 'File'
export type Theme = 'system' | 'dark' | 'light'

export type SmartKind =
  | { type: 'plain' }
  | { type: 'link'; domain: string }
  | { type: 'color'; value: string }
  | { type: 'code' }

export type Preview =
  | { kind: 'text'; text: string; smart: SmartKind }
  | { kind: 'image'; dataUrl: string; width: number; height: number }
  | { kind: 'file'; name: string }

export interface Peer {
  deviceId: string
  name: string
  os: Os
  fingerprint: string
  addresses: string[]
  mode: DeliveryMode
}

export interface PeerStatus {
  peer: Peer
  online: boolean
  addr: string | null
}

export interface NearbyDevice {
  device: string
  name: string
  os: Os
  addresses: string[]
}

export interface Offer {
  peer: string
  itemId: string
  kind: ItemKind
  size: number
  preview: Preview
  ageMs: number
}

export interface HistoryEntry {
  id: string
  source: string
  kind: ItemKind
  createdMs: number
  size: number
  preview: Preview
}

export interface Snapshot {
  deviceId: string
  deviceName: string
  platform: string
  port: number
  peers: PeerStatus[]
  nearby: NearbyDevice[]
  inbox: Offer[]
  hotkeyError?: string | null
  clipboardError?: { seq: number; message: string } | null
}

export interface AppSettings {
  deviceName: string | null
  saveDir: string | null
  hotkey: string
  autostart: boolean
  theme: Theme
  onboarded: boolean
}

export type EngineEvent =
  | { type: 'engineRestarted' }
  | { type: 'peerOnline'; peer: string; name: string; addr: string }
  | { type: 'peerOffline'; peer: string }
  | { type: 'discovered'; device: string; name: string; os: Os; addresses: string[] }
  | { type: 'identityChanged'; peer: string }
  | { type: 'paired'; peer: string; name: string }
  | { type: 'pairingFailed'; reason: string }
  | { type: 'localItem'; itemId: string }
  | ({ type: 'offerReceived' } & Offer)
  | { type: 'clipboardApplied'; itemId: string; from: string }
  | { type: 'transferProgress'; transferId: string; peer: string; name: string; done: number; total: number; outgoing: boolean }
  | { type: 'transferPaused'; transferId: string; peer: string }
  | { type: 'transferDone'; transferId: string; peer: string; path: string | null }
  | { type: 'transferFailed'; transferId: string; peer: string; reason: string }
  | { type: 'error'; message: string; seq?: number }
  | { type: 'inboxChanged' }
  | { type: 'peersChanged' }
