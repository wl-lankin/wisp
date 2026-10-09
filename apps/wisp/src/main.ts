import { createApp, type Component } from 'vue'
import './styles.css'
import PanelView from './views/PanelView.vue'
import ToastView from './views/ToastView.vue'
import TimelineView from './views/TimelineView.vue'
import SettingsView from './views/SettingsView.vue'
import OnboardingView from './views/OnboardingView.vue'

export const views: Record<string, Component> = {
  panel: PanelView,
  toast: ToastView,
  timeline: TimelineView,
  settings: SettingsView,
  onboarding: OnboardingView,
}

export function viewFor(search: string): Component {
  const name = new URLSearchParams(search).get('view') ?? 'panel'
  return views[name] ?? PanelView
}

document.documentElement.dataset.view = new URLSearchParams(location.search).get('view') ?? 'panel'
// Windows 11 draws its own rounded frame around the undecorated panel and onboarding windows.
document.documentElement.dataset.os = navigator.userAgent.includes('Windows') ? 'windows' : 'other'
createApp(viewFor(location.search)).mount('#app')
