<script setup lang="ts">
import { computed } from 'vue'
import hljs from 'highlight.js/lib/common'
import IconGlyph from './IconGlyph.vue'
import type { Preview } from '../lib/types'

const props = defineProps<{ preview: Preview; compact?: boolean }>()

const highlighted = computed(() =>
  props.preview.kind === 'text' && props.preview.smart.type === 'code'
    ? hljs.highlightAuto(props.preview.text).value
    : '',
)
</script>

<template>
  <div class="min-w-0 text-[13px] leading-snug">
    <template v-if="preview.kind === 'image'">
      <img
        :src="preview.dataUrl"
        :width="preview.width"
        :height="preview.height"
        class="max-h-40 w-auto max-w-full rounded-lg border border-[var(--border)] object-contain"
        alt="Image preview"
        draggable="false"
      />
    </template>
    <template v-else-if="preview.kind === 'file'">
      <div class="flex items-center gap-2"><IconGlyph name="file" /> <span class="truncate">{{ preview.name }}</span></div>
    </template>
    <template v-else-if="preview.smart.type === 'color'">
      <div class="flex items-center gap-3">
        <span data-swatch class="size-9 rounded-lg border border-[var(--border)]" :style="{ '--swatch': preview.smart.value, background: 'var(--swatch)' }" />
        <span class="font-mono">{{ preview.smart.value }}</span>
      </div>
    </template>
    <template v-else-if="preview.smart.type === 'link'">
      <div class="flex items-center gap-2 text-[var(--accent-a)]"><IconGlyph name="link" /> <span class="font-medium">{{ preview.smart.domain }}</span></div>
      <p class="mt-1 truncate text-[var(--muted)]">{{ preview.text }}</p>
    </template>
    <template v-else-if="preview.smart.type === 'code'">
      <pre class="max-h-36 overflow-hidden rounded-lg bg-[var(--surface)] p-2 font-mono text-[11.5px]"><code class="hljs" v-html="highlighted" /></pre>
    </template>
    <p v-else class="whitespace-pre-wrap break-words" :class="compact ? 'line-clamp-2' : 'line-clamp-6'">{{ preview.text }}</p>
  </div>
</template>

<style>
.hljs-keyword, .hljs-built_in { color: var(--accent-b); }
.hljs-string, .hljs-attr { color: var(--accent-a); }
.hljs-comment { color: var(--muted); font-style: italic; }
.hljs-number, .hljs-literal { color: #f5b971; }
</style>
