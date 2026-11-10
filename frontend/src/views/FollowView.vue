<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'
import RoomCard from '../components/RoomCard.vue'
import type { Follow, SiteInfo } from '../types'

const router = useRouter()
const items = ref<Follow[]>([])
const sites = ref<SiteInfo[]>([])
const filter = ref<'all' | 'live' | 'off'>('all')
const input = ref('')
const platform = ref('bilibili')
const loading = ref(false)
const refreshing = ref(false)
const err = ref('')

const shown = computed(() => {
  if (filter.value === 'live') return items.value.filter((f) => f.live)
  if (filter.value === 'off') return items.value.filter((f) => !f.live)
  return items.value
})
const liveCount = computed(() => items.value.filter((f) => f.live).length)

async function load() {
  loading.value = true
  err.value = ''
  try {
    items.value = (await api.follows()).items
    if (!sites.value.length) sites.value = (await api.sites()).sites
  } catch (e) { err.value = (e as Error).message }
  finally { loading.value = false }
}

async function refresh() {
  refreshing.value = true
  err.value = ''
  try {
    await Promise.all(items.value.map(async (f) => {
      try {
        const s = await api.status(f.platform, f.roomId)
        f.live = s.live
      } catch { /* 单个失败不影响其他 */ }
    }))
  } finally { refreshing.value = false }
}

async function add() {
  err.value = ''
  const text = input.value.trim()
  if (!text) return
  try {
    let p = platform.value
    let roomId = text
    if (text.includes('http') || text.includes('.com') || text.includes(' ')) {
      const r = await api.parse(text)
      p = r.platform
      roomId = r.roomId
    }
    if (!roomId) throw new Error('未能识别房间号')
    let title = '', cover = '', userName = ''
    try {
      const { detail } = await api.room(p, roomId)
      title = detail.title; cover = detail.cover; userName = detail.userName
    } catch { /* 详情失败也允许先加入 */ }
    await api.addFollow({ platform: p, roomId, title, cover, userName })
    input.value = ''
    await load()
    await refresh()
  } catch (e) { err.value = (e as Error).message }
}

async function remove(id: string) {
  err.value = ''
  try { await api.delFollow(id); await load() } catch (e) { err.value = (e as Error).message }
}

async function toggleTop(f: Follow) {
  err.value = ''
  try { await api.topFollow(f.id, !f.top); await load() } catch (e) { err.value = (e as Error).message }
}

function open(f: Follow) {
  router.push({ name: 'player', query: { platform: f.platform, room: f.roomId } })
}

onMounted(async () => { await load(); await refresh() })
</script>

<template>
  <div class="page">
    <h1 class="page-title">
      关注 <small>共 {{ items.length }} 个 · {{ liveCount }} 个直播中</small>
    </h1>

    <div class="panel" style="margin-bottom: 16px">
      <div class="row">
        <select class="select" style="max-width: 170px" v-model="platform">
          <option v-for="s in sites" :key="s.id" :value="s.id">{{ s.name }}</option>
        </select>
        <input class="input" v-model="input" placeholder="房间号，或直接粘贴分享链接 / 短链" @keyup.enter="add" />
        <button class="btn primary" :disabled="!input.trim() || loading" @click="add">添加关注</button>
      </div>
      <p class="hint" style="margin: 10px 0 0">
        粘贴链接时会自动识别平台；纯房间号请使用左侧选择的平台。数据保存在服务端，多设备打开同一地址即可共享。
      </p>
    </div>

    <div v-if="err" class="error" style="margin-bottom: 14px">{{ err }}</div>

    <div class="tabs">
      <button :class="['btn', 'sm', filter === 'all' ? 'active' : '']" @click="filter = 'all'">全部</button>
      <button :class="['btn', 'sm', filter === 'live' ? 'active' : '']" @click="filter = 'live'">直播中</button>
      <button :class="['btn', 'sm', filter === 'off' ? 'active' : '']" @click="filter = 'off'">未开播</button>
      <div class="spacer"></div>
      <button class="btn sm" :disabled="refreshing" @click="refresh">{{ refreshing ? '刷新中…' : '刷新开播状态' }}</button>
    </div>

    <div v-if="loading" class="hint">加载中…</div>
    <div v-else-if="!shown.length" class="panel hint" style="text-align: center; padding: 40px">
      暂无关注。可在上方添加，或到「搜索」页寻找直播间。
    </div>
    <div v-else class="grid">
      <RoomCard
        v-for="f in shown" :key="f.id"
        :title="f.title" :cover="f.cover" :user="f.userName" :live="f.live"
        @click="open(f)"
      >
        <template #actions>
          <button class="btn sm ghost" @click="toggleTop(f)">{{ f.top ? '取消置顶' : '置顶' }}</button>
          <button class="btn sm ghost danger" @click="remove(f.id)">删除</button>
        </template>
      </RoomCard>
    </div>
  </div>
</template>
