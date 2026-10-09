<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'

withDefaults(defineProps<{ maximizable?: boolean }>(), { maximizable: false })

// macOS keeps its own traffic lights over the content; Windows gets drawn controls.
const isMac = navigator.userAgent.includes('Mac')

const minimize = () => getCurrentWindow().minimize()
const toggleMaximize = () => getCurrentWindow().toggleMaximize()
const close = () => getCurrentWindow().close()
</script>

<template>
  <div data-tauri-drag-region class="window-bar sticky top-0 z-20 flex h-9 shrink-0 items-stretch justify-end bg-[var(--bg)]">
    <template v-if="!isMac">
      <button data-testid="window-minimize" class="window-button hoverable" title="Minimize" @click="minimize">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" stroke="currentColor" /></svg>
      </button>
      <button v-if="maximizable" data-testid="window-maximize" class="window-button hoverable" title="Maximize" @click="toggleMaximize">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" /></svg>
      </button>
      <button data-testid="window-close" class="window-button window-close hoverable" title="Close" @click="close">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" /></svg>
      </button>
    </template>
  </div>
</template>

<style scoped>
.window-button {
  display: grid;
  place-items: center;
  width: 46px;
  color: var(--muted);
}

.window-button:hover {
  background: var(--surface);
  color: var(--text);
}

.window-close:hover {
  background: #e81123;
  color: #fff;
}
</style>
