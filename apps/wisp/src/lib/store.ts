import type { EngineEvent, NearbyDevice, Offer, PeerStatus, Snapshot } from './types'

export interface Transfer {
  id: string
  peer: string
  name: string
  done: number
  total: number
  outgoing: boolean
  status: 'active' | 'paused' | 'done' | 'failed'
  path?: string | null
  reason?: string
}

export interface Notice {
  id: number
  kind: 'error' | 'warning' | 'info'
  text: string
}

export interface State {
  ready: boolean
  deviceId: string
  deviceName: string
  platform: string
  port: number
  peers: PeerStatus[]
  nearby: NearbyDevice[]
  inbox: Offer[]
  transfers: Record<string, Transfer>
  notices: Notice[]
  identityWarnings: string[]
  historyVersion: number
}

const MAX_NOTICES = 5
let noticeSeq = 0

export function initialState(): State {
  return {
    ready: false,
    deviceId: '',
    deviceName: '',
    platform: '',
    port: 0,
    peers: [],
    nearby: [],
    inbox: [],
    transfers: {},
    notices: [],
    identityWarnings: [],
    historyVersion: 0,
  }
}

export function applySnapshot(s: State, snap: Snapshot): void {
  s.ready = true
  s.deviceId = snap.deviceId
  s.deviceName = snap.deviceName
  s.platform = snap.platform
  s.port = snap.port
  s.peers = snap.peers
  s.nearby = snap.nearby.filter((n) => !snap.peers.some((p) => p.peer.deviceId === n.device))
  s.inbox = snap.inbox
}

function notify(s: State, kind: Notice['kind'], text: string): void {
  s.notices.push({ id: ++noticeSeq, kind, text })
  if (s.notices.length > MAX_NOTICES) s.notices.splice(0, s.notices.length - MAX_NOTICES)
}

export function dismissNotice(s: State, id: number): void {
  s.notices = s.notices.filter((n) => n.id !== id)
}

function peerName(s: State, id: string): string {
  return s.peers.find((p) => p.peer.deviceId === id)?.peer.name ?? 'A device'
}

function transfer(s: State, id: string, peer: string): Transfer {
  s.transfers[id] ??= { id, peer, name: '', done: 0, total: 0, outgoing: false, status: 'active' }
  return s.transfers[id]
}

/** Applies one engine event. Returns true when the caller should reload the snapshot. */
export function reduce(s: State, e: EngineEvent): boolean {
  switch (e.type) {
    case 'engineRestarted':
      for (const id of Object.keys(s.transfers)) {
        const status = s.transfers[id].status
        if (status === 'active' || status === 'paused') delete s.transfers[id]
      }
      s.identityWarnings = []
      return true
    case 'peerOnline': {
      const p = s.peers.find((x) => x.peer.deviceId === e.peer)
      if (!p) return true
      p.online = true
      p.addr = e.addr
      return false
    }
    case 'peerOffline': {
      const p = s.peers.find((x) => x.peer.deviceId === e.peer)
      if (p) {
        p.online = false
        p.addr = null
      }
      return false
    }
    case 'discovered': {
      if (s.peers.some((p) => p.peer.deviceId === e.device)) return false
      const entry = { device: e.device, name: e.name, os: e.os, addresses: e.addresses }
      const i = s.nearby.findIndex((n) => n.device === e.device)
      if (i >= 0) s.nearby[i] = entry
      else s.nearby.push(entry)
      return false
    }
    case 'identityChanged': {
      if (s.identityWarnings.includes(e.peer)) return false
      s.identityWarnings.push(e.peer)
      notify(s, 'warning', `${peerName(s, e.peer)} announced a different identity. If you did not reinstall it, ignore it and keep the old pairing.`)
      return false
    }
    case 'paired':
      s.nearby = s.nearby.filter((n) => n.device !== e.peer)
      return true
    case 'pairingFailed':
      notify(s, 'error', e.reason)
      return false
    case 'localItem':
      s.historyVersion++
      return false
    case 'offerReceived': {
      const { type: _type, ...offer } = e
      s.inbox = [offer, ...s.inbox.filter((o) => o.itemId !== offer.itemId)]
      return false
    }
    case 'clipboardApplied':
      s.inbox = s.inbox.filter((o) => o.itemId !== e.itemId)
      s.historyVersion++
      return false
    case 'transferProgress': {
      const t = transfer(s, e.transferId, e.peer)
      if (t.status === 'done' || t.status === 'failed') return false
      Object.assign(t, { name: e.name || t.name, done: e.done, total: e.total, outgoing: e.outgoing, status: 'active' })
      return false
    }
    case 'transferPaused':
      transfer(s, e.transferId, e.peer).status = 'paused'
      return false
    case 'transferDone':
      Object.assign(transfer(s, e.transferId, e.peer), { status: 'done', path: e.path })
      return false
    case 'transferFailed':
      Object.assign(transfer(s, e.transferId, e.peer), { status: 'failed', reason: e.reason })
      return false
    case 'error':
      notify(s, 'error', e.message)
      return false
  }
}

/** Puts offers back at the front of the inbox, newest first, without duplicates. */
export function mergeOffers(s: State, offers: Offer[]): void {
  if (offers.length === 0) return
  const ids = new Set(offers.map((o) => o.itemId))
  s.inbox = [...offers, ...s.inbox.filter((o) => !ids.has(o.itemId))]
}

export interface Resyncer {
  request: () => Promise<void>
  track: (e: EngineEvent) => void
}

/**
 * Single-flight snapshot loader. Calls while a load is running collapse into one follow-up run.
 * Offers that arrive while a snapshot is in flight are handed to `apply` so they are not wiped.
 */
export function createResyncer<T>(
  fetch: () => Promise<T>,
  apply: (snap: T, inFlightOffers: Offer[]) => void,
  onGiveUp: () => void,
  opts: { attempts?: number; delayMs?: number } = {},
): Resyncer {
  const attempts = opts.attempts ?? 40
  const delayMs = opts.delayMs ?? 250
  let running: Promise<void> | null = null
  let dirty = false
  let seen: Offer[] = []

  async function load(): Promise<boolean> {
    for (let attempt = 0; attempt < attempts; attempt++) {
      seen = []
      try {
        const snap = await fetch()
        apply(snap, seen)
        return true
      } catch {
        if (attempt < attempts - 1) await new Promise((r) => setTimeout(r, delayMs))
      }
    }
    return false
  }

  async function loop(): Promise<void> {
    try {
      do {
        dirty = false
        if (!(await load())) {
          dirty = false
          onGiveUp()
          return
        }
      } while (dirty)
    } finally {
      running = null
    }
  }

  return {
    request() {
      if (running) {
        dirty = true
        return running
      }
      running = loop()
      return running
    },
    track(e) {
      if (!running) return
      if (e.type === 'offerReceived') {
        const { type: _type, ...offer } = e
        seen = [offer, ...seen.filter((o) => o.itemId !== offer.itemId)]
      } else if (e.type === 'clipboardApplied') {
        seen = seen.filter((o) => o.itemId !== e.itemId)
      }
    },
  }
}
