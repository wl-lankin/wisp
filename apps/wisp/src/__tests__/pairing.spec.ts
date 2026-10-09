import { describe, expect, it } from 'vitest'
import { findNewPeers, formatCode, friendlyPairingError, normalizeCode } from '../lib/pairing'
import type { PeerStatus } from '../lib/types'

function peerStatus(deviceId: string, name: string): PeerStatus {
  return { peer: { deviceId, name, os: 'MacOs', fingerprint: 'ab', addresses: [], mode: 'Catch' }, online: true, addr: null }
}

describe('pairing codes', () => {
  it('accepts six digits with any spacing', () => {
    expect(normalizeCode('123 456')).toBe('123456')
    expect(normalizeCode(' 12-34-56 ')).toBe('123456')
    expect(normalizeCode('12345')).toBeNull()
    expect(normalizeCode('1234567')).toBeNull()
    expect(normalizeCode('abcdef')).toBeNull()
  })
  it('groups codes for reading', () => {
    expect(formatCode('123456')).toBe('123 456')
  })
})

describe('findNewPeers', () => {
  it('returns peers whose id was not known when the code was created', () => {
    const known = new Set(['a'])
    const peers = [peerStatus('a', 'Old'), peerStatus('b', 'New')]
    expect(findNewPeers(peers, known).map((p) => p.peer.name)).toEqual(['New'])
  })
  it('does not depend on the list length or identity', () => {
    const known = new Set(['a', 'b'])
    expect(findNewPeers([peerStatus('b', 'B')], known)).toEqual([])
    expect(findNewPeers([peerStatus('c', 'C'), peerStatus('a', 'A')], known).map((p) => p.peer.name)).toEqual(['C'])
  })
})

describe('friendlyPairingError', () => {
  it('explains a wrong code', () => {
    expect(friendlyPairingError('Error: pairing: wrong code')).toMatch(/code did not match/i)
  })
  it('explains too many wrong codes', () => {
    expect(friendlyPairingError('Too many wrong codes. Start pairing again.')).toMatch(/too many/i)
  })
  it('explains an unreachable address', () => {
    for (const raw of ['transport: connection lost', 'pairing: timed out', 'io: connection refused (os error 111)']) {
      expect(friendlyPairingError(raw)).toMatch(/could not reach/i)
    }
  })
  it('keeps already friendly messages and strips the Error prefix', () => {
    expect(friendlyPairingError(new Error('"abc" is not an IP address'))).toBe('"abc" is not an IP address')
  })
})
