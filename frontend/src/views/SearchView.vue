<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'
import RoomCard from '../components/RoomCard.vue'
import type { SearchItem, SiteInfo } from '../types'

const router = useRouter()
const sites = ref<SiteInfo[]>([])
const platform = ref('bilibili')
const kw = ref('')
const anchor = ref(false)
const page = ref(1)
const items = ref<SearchItem[]>([])
const hasMore = ref(false)
const loading = ref(false)
const err = ref('')
const msg = ref('')
const searched = ref(false)

const searchable = computed(() => sites.value.filter((s) => s.searchable))
const currentName = computed(() => sites.value.find((s) => s.id === platform.value)?.name || platform.value)

onMounted(async () => {
  try {
    sites.value = (await api.sites()).sites
    platform.value = searchable.value[0]?.id || 'bilibili'
  } catch (e) { err.value = (e as Error).message }
})

watch(platform, () => { items.value = []; searched.value = false })

async function doSearch(nextPage = 1) {
  if (!kw.value.trim()) return
  loading.value = true
  err.value = ''; msg.value = ''
  page.value = nextPage
  searched.value = true
  try {
    const r = await api.search(platform.value, kw.value.trim(), page.value, anchor.value)
    items.value = r.items
    hasMore.value = r.hasMore
  } catch (e) { err.value = (e as Error).message }
  finally { loading.value = false }
}

function open(it: SearchItem) {
  router.push({ name: 'player', query: { platform: platform.value, room: it.roomId } })
}

async function follow(it: SearchItem) {
  err.value = ''; msg.value = ''
  try {
    await api.addFollow({
      platform: platform.value, roomId: it.roomId,
      title: it.title, cover: it.cover, userName: it.userName,
    })
    msg.value = `已关注「${it.userName || it.roomId}」`
  } catch (e) { err.value = (e as Error).message }
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div>
        <h1 class="page-title">搜索 <small>{{ currentName }}</small></h1>
        <div class="sub">按关键词或主播名查找直播间，找到后可直接播放或加入关注</div>
      </div>
    </div>

    <div class="panel" style="margin-bottom: 18px">
      <div class="panel-head" style="margin-bottom: 14px">
        <span class="ico">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="6.5" /><path d="M16 16l4.5 4.5" /></svg>
        </span>
        <h2>找直播间</h2>
      </div>
      <div class="row">
        <select class="select" style="max-width: 180px" v-model="platform">
          <option v-for="s in searchable" :key="s.id" :value="s.id">{{ s.name }}</option>
        </select>
        <input class="input" v-model="kw" placeholder="关键词 / 主播名" @keyup.enter="doSearch(1)" />
        <button class="btn primary" :disabled="!kw.trim() || loading" @click="doSearch(1)">
          {{ loading ? '搜索中…' : '搜索' }}
        </button>
      </div>
      <div class="tabs" style="margin: 13px 0 0">
        <button :class="['btn', 'sm', !anchor ? 'active' : '']" @click="anchor = false">直播间</button>
        <button :class="['btn', 'sm', anchor ? 'active' : '']" @click="anchor = true">主播</button>
      </div>
    </div>

    <div v-if="err" class="error" style="margin-bottom: 14px">{{ err }}</div>
    <div v-if="msg" class="tag ok" style="margin-bottom: 14px">{{ msg }}</div>

    <div v-if="loading" class="hint">搜索中…</div>
    <div v-else-if="searched && !items.length" class="panel empty-state">
      <div class="bubble">
        <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="6.5" /><path d="M16 16l4.5 4.5" /></svg>
      </div>
      <div class="t">没有找到相关结果</div>
      <div class="hint">换个关键词试试；抖音搜索建议在「设置」中填写 Cookie</div>
    </div>
    <div v-else class="grid">
      <RoomCard
        v-for="(it, i) in items" :key="`${it.roomId}-${i}`"
        :title="it.title || it.userName" :cover="it.cover"
        :user="it.userName || it.roomId" :live="it.liveStatus"
        :subtitle="currentName"
        @click="open(it)"
      >
        <template #actions>
          <button class="btn sm ghost" @click="follow(it)">关注</button>
          <button class="btn sm ghost" @click="open(it)">播放</button>
        </template>
      </RoomCard>
    </div>

    <div v-if="items.length" class="row" style="margin-top: 20px; justify-content: center">
      <button class="btn" :disabled="page <= 1" @click="doSearch(page - 1)">上一页</button>
      <span class="muted">第 {{ page }} 页</span>
      <button class="btn" :disabled="!hasMore" @click="doSearch(page + 1)">下一页</button>
    </div>
  </div>
</template>
