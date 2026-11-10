<script setup lang="ts">
import { computed, ref } from 'vue'
import { proxyImage } from '../types'

const props = defineProps<{
  title: string
  cover: string
  user: string
  online?: number
  live?: boolean
}>()
const emit = defineEmits<{ (e: 'click'): void }>()
const broken = ref(false)
const src = computed(() => (props.cover ? proxyImage(props.cover) : ''))
</script>

<template>
  <div class="card room-card" @click="emit('click')">
    <img v-if="src && !broken" class="room-cover" :src="src" referrerpolicy="no-referrer" @error="broken = true" />
    <div v-else class="room-cover placeholder">📺</div>
    <div class="room-body">
      <div class="room-title" :title="title">{{ title || '未命名直播间' }}</div>
      <div class="room-meta">
        <span :title="user">{{ user || '—' }}</span>
        <span :class="['tag', live ? 'live' : '']">{{ live ? '直播中' : '未开播' }}</span>
      </div>
    </div>
    <div class="room-actions" @click.stop><slot name="actions" /></div>
  </div>
</template>
