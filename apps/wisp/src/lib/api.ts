import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { AppSettings, DeliveryMode, EngineEvent, HistoryEntry, ItemKind, Offer, Peer, Snapshot } from './types'

export type WindowName = 'panel' | 'toast' | 'timeline' | 'settings' | 'onboarding'

export const api = {
  snapshot: () => invoke<Snapshot>('snapshot'),
  getSettings: () => invoke<AppSettings>('get_settings'),
  updateSettings: (next: AppSettings) => invoke<AppSettings>('update_settings', { next }),
  completeOnboarding: (deviceName: string) => invoke<void>('complete_onboarding', { deviceName }),
  renameDevice: (name: string) => invoke<string>('rename_device', { name }),
  historyCount: () => invoke<number>('history_count'),
  defaultDeviceName: () => invoke<string>('default_device_name'),
  pickFolder: () => invoke<string | null>('pick_folder'),
  beginPairing: () => invoke<string>('begin_pairing'),
  cancelPairing: () => invoke<void>('cancel_pairing'),
  pairWith: (addr: string, code: string) => invoke<Peer>('pair_with', { addr, code }),
  setMode: (peer: string, mode: DeliveryMode) => invoke<void>('set_mode', { peer, mode }),
  forget: (peer: string) => invoke<void>('forget', { peer }),
  catchItem: (itemId: string) => invoke<void>('catch', { itemId }),
  catchLatest: () => invoke<string | null>('catch_latest'),
  dismiss: (itemId: string) => invoke<void>('dismiss', { itemId }),
  saveOffer: (itemId: string) => invoke<string>('save_offer', { itemId }),
  history: (q: { text?: string; source?: string; kind?: ItemKind; limit?: number }) =>
    invoke<HistoryEntry[]>('history', q),
  copyFromHistory: (id: string) => invoke<void>('copy_from_history', { id }),
  exportItem: (id: string) => invoke<string>('export_item', { id }),
  sendClipboardTo: (peer: string) => invoke<void>('send_clipboard_to', { peer }),
  sendFiles: (peer: string, paths: string[]) => invoke<string[]>('send_files', { peer, paths }),
  addAddress: (peer: string, addr: string) => invoke<void>('add_address', { peer, addr }),
  openWindow: (name: WindowName) => invoke<void>('open_window', { name }),
  hideWindow: (name: WindowName) => invoke<void>('hide_window', { name }),
  fitToast: (height: number) => invoke<void>('fit_toast', { height }),
}

export type { Offer }

export function onEngineEvent(cb: (e: EngineEvent) => void): Promise<UnlistenFn> {
  return listen<EngineEvent>('wisp://event', (msg) => cb(msg.payload))
}

export function onSettings(cb: (s: AppSettings) => void): Promise<UnlistenFn> {
  return listen<AppSettings>('wisp://settings', (msg) => cb(msg.payload))
}
