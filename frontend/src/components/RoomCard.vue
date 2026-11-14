<script setup lang="ts">
import { computed, ref } from 'vue'
import { proxyImage } from '../types'

const props = defineProps<{
  title: string
  cover: string
  user: string
  online?: number
  live?: boolean
  subtitle?: string
  top?: boolean
}>()
const emit = defineEmits<{ (e: 'click'): void }>()
const broken = ref(false)
const src = computed(() => (props.cover ? proxyImage(props.cover) : ''))
/** 没有标题时用主播名作为主标题，不再显示「未命名直播间」占位文字 */
const mainTitle = computed(() => props.title || props.user || '')
const hue = computed(() => {
  const s = `${props.title}${props.user}`
  let h = 0
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 360
  return h
})
</script>

<template>
  <div class="card room-card" @click="emit('click')">
    <div class="cover-box" style="position: relative">
      <img v-if="src && !broken" class="room-cover" :src="src" referrerpolicy="no-referrer" @error="broken = true" />
      <div v-else class="room-cover placeholder" :style="{ '--h': hue }">
        <svg width="42" height="42" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <rect x="2.5" y="5.5" width="19" height="13.5" rx="2.5" />
          <path d="M10 9.5l5 2.8-5 2.8z" fill="currentColor" stroke="none" />
        </svg>
      </div>
      <div class="cover-shade"></div>
      <span v-if="live" class="cover-live"><i class="dot"></i>LIVE</span>
      <span v-if="top" class="tag accent" style="position: absolute; z-index: 2; top: 10px; right: 10px">置顶</span>
    </div>
    <div class="room-body">
      <div v-if="mainTitle" class="room-title" :title="mainTitle">{{ mainTitle }}</div>
      <div class="room-meta">
        <span :title="title ? user : (subtitle || '')">{{ title ? (user || '—') : (subtitle || '') }}</span>
        <span :class="['tag', live ? 'live' : '']">
          <i v-if="live" class="status-dot"></i>{{ live ? '直播中' : '未开播' }}
        </span>
      </div>
    </div>
    <div class="room-actions" @click.stop><slot name="actions" /></div>
  </div>
</template>
