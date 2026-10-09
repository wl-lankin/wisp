import { describe, expect, it } from 'vitest'
import { applySnapshot, initialState, reduce } from '../lib/store'
import type { Offer, Snapshot } from '../lib/types'

const mac = '11111111-1111-1111-1111-111111111111'
const pc = '22222222-2222-2222-2222-222222222222'

function snap(): Snapshot {
  return {
    deviceId: pc,
    deviceName: 'PC',
    platform: 'windows',
    port: 47800,
    peers: [
      {
        peer: { deviceId: mac, name: 'MacBook', os: 'MacOs', fingerprint: 'ab', addresses: [], mode: 'Catch' },
        online: false,
        addr: null,
      },
    ],
    nearby: [],
    inbox: [],
  }
}

function offer(itemId: string): Offer {
  return { peer: mac, itemId, kind: 'Text', size: 5, preview: { kind: 'text', text: 'hello', smart: { type: 'plain' } }, ageMs: 0 }
}

describe('reduce', () => {
  it('tracks peers going online and offline', () => {
    const s = initialState()
    applySnapshot(s, snap())
    expect(s.ready).toBe(true)
    reduce(s, { type: 'peerOnline', peer: mac, name: 'MacBook', addr: '192.168.1.5:47800' })
    expect(s.peers[0].online).toBe(true)
    expect(s.peers[0].addr).toBe('192.168.1.5:47800')
    reduce(s, { type: 'peerOffline', peer: mac })
    expect(s.peers[0].online).toBe(false)
  })

  it('asks for a resync when an unknown peer appears or pairing completes', () => {
    const s = initialState()
    applySnapshot(s, snap())
    expect(reduce(s, { type: 'peerOnline', peer: 'new', name: 'X', addr: '1.2.3.4:1' })).toBe(true)
    expect(reduce(s, { type: 'paired', peer: 'new', name: 'X' })).toBe(true)
    expect(reduce(s, { type: 'peerOffline', peer: mac })).toBe(false)
  })

  it('asks for a resync when the engine (re)starts', () => {
    const s = initialState()
    expect(reduce(s, { type: 'engineRestarted' })).toBe(true)
    expect(reduce(s, { type: 'engineRestarted' })).toBe(true)
  })

  it('keeps the inbox newest first without duplicates and drops caught items', () => {
    const s = initialState()
    applySnapshot(s, snap())
    reduce(s, { type: 'offerReceived', ...offer('a') })
    reduce(s, { type: 'offerReceived', ...offer('b') })
    reduce(s, { type: 'offerReceived', ...offer('a') })
    expect(s.inbox.map((o) => o.itemId)).toEqual(['a', 'b'])
    reduce(s, { type: 'clipboardApplied', itemId: 'a', from: mac })
    expect(s.inbox.map((o) => o.itemId)).toEqual(['b'])
    expect(s.historyVersion).toBe(1)
  })

  it('shows an identity warning once per device', () => {
    const s = initialState()
    applySnapshot(s, snap())
    for (let i = 0; i < 5; i++) reduce(s, { type: 'identityChanged', peer: mac })
    expect(s.notices.filter((n) => n.kind === 'warning')).toHaveLength(1)
    expect(s.notices[0].text).toContain('MacBook')
  })

  it('lists nearby devices that are not paired yet', () => {
    const s = initialState()
    applySnapshot(s, snap())
    reduce(s, { type: 'discovered', device: 'x', name: 'Laptop', os: 'Windows', addresses: ['10.0.0.2:47800'] })
    reduce(s, { type: 'discovered', device: mac, name: 'MacBook', os: 'MacOs', addresses: [] })
    expect(s.nearby.map((n) => n.name)).toEqual(['Laptop'])
  })

  it('follows a transfer from progress to done or failed', () => {
    const s = initialState()
    reduce(s, { type: 'transferProgress', transferId: 't', peer: mac, name: 'a.pdf', done: 10, total: 100, outgoing: false })
    expect(s.transfers.t.status).toBe('active')
    reduce(s, { type: 'transferPaused', transferId: 't', peer: mac })
    expect(s.transfers.t.status).toBe('paused')
    reduce(s, { type: 'transferDone', transferId: 't', peer: mac, path: 'C:/x/a.pdf' })
    expect(s.transfers.t.status).toBe('done')
    expect(s.transfers.t.path).toBe('C:/x/a.pdf')
    reduce(s, { type: 'transferFailed', transferId: 'u', peer: mac, reason: 'nope' })
    expect(s.transfers.u.status).toBe('failed')
  })

  it('caps notices at five', () => {
    const s = initialState()
    for (let i = 0; i < 8; i++) reduce(s, { type: 'error', message: `e${i}` })
    expect(s.notices).toHaveLength(5)
    expect(s.notices[4].text).toBe('e7')
  })

  it('forgets in-flight transfers and identity warnings when the engine restarts', () => {
    const s = initialState()
    applySnapshot(s, snap())
    reduce(s, { type: 'transferProgress', transferId: 'a', peer: mac, name: 'a', done: 1, total: 2, outgoing: false })
    reduce(s, { type: 'transferPaused', transferId: 'p', peer: mac })
    reduce(s, { type: 'transferDone', transferId: 'd', peer: mac, path: null })
    reduce(s, { type: 'identityChanged', peer: mac })
    expect(reduce(s, { type: 'engineRestarted' })).toBe(true)
    expect(Object.keys(s.transfers)).toEqual(['d'])
    expect(s.identityWarnings).toEqual([])
    reduce(s, { type: 'identityChanged', peer: mac })
    expect(s.notices.filter((n) => n.kind === 'warning')).toHaveLength(2)
  })

  it('does not reopen a finished transfer on late progress', () => {
    const s = initialState()
    reduce(s, { type: 'transferDone', transferId: 't', peer: mac, path: 'x' })
    reduce(s, { type: 'transferProgress', transferId: 't', peer: mac, name: 'a', done: 1, total: 2, outgoing: false })
    expect(s.transfers.t.status).toBe('done')
    reduce(s, { type: 'transferFailed', transferId: 'u', peer: mac, reason: 'r' })
    reduce(s, { type: 'transferProgress', transferId: 'u', peer: mac, name: 'a', done: 1, total: 2, outgoing: false })
    expect(s.transfers.u.status).toBe('failed')
  })
})
