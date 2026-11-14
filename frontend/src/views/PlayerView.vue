<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { api } from '../api'
import LivePlayer from '../components/LivePlayer.vue'
import DanmakuOverlay from '../components/DanmakuOverlay.vue'
import type { PlayInfo, RoomDetail } from '../types'

const route = useRoute()
const router = useRouter()
const platform = computed(() => String(route.query.platform || ''))
const room = computed(() => String(route.query.room || ''))

const detail = ref<RoomDetail | null>(null)
const playInfo = ref<PlayInfo | null>(null)
const qi = ref(0)
const li = ref(0)
const danmakuOn = ref(true)
const online = ref(0)
const logs = ref<string[]>([])
const err = ref('')
const msg = ref('')
const loading = ref(false)

const quality = computed(() => playInfo.value?.qualities[qi.value] || null)
const line = computed(() => quality.value?.lines[li.value] || null)

async function load() {
  if (!platform.value || !room.value) { err.value = '缺少平台或房间号'; return }
  loading.value = true
  err.value = ''; msg.value = ''
  playInfo.value = null; detail.value = null
  qi.value = 0; li.value = 0; online.value = 0; logs.value = []
  try {
    detail.value = (await api.room(platform.value, room.value)).detail
    playInfo.value = await api.play(platform.value, room.value)
    if (!playInfo.value.qualities.length) msg.value = '主播未开播或暂未获取到播放地址'
  } catch (e) { err.value = (e as Error).message }
  finally { loading.value = false }
}

function setQuality(i: number) { qi.value = i; li.value = 0 }
function addLog(t: string) {
  logs.value.push(t)
  if (logs.value.length > 80) logs.value.shift()
}
function onOnline(n: number) { online.value = n }
function goDanmakuSettings() {
  router.push({ name: 'settings', query: { section: 'danmaku' } })
}

async function addFollow() {
  err.value = ''; msg.value = ''
  try {
    const d = detail.value
    await api.addFollow({
      platform: platform.value, roomId: room.value,
      title: d?.title || '', cover: d?.cover || '', userName: d?.userName || '',
    })
    msg.value = '已加入关注'
  } catch (e) { err.value = (e as Error).message }
}

watch([platform, room], load)
onMounted(load)
</script>

<template>
  <div class="page">
    <div class="page-head" style="margin-bottom: 16px">
      <button class="btn sm ghost" @click="router.back()">← 返回</button>
      <div>
        <h1 class="page-title" style="margin: 0; font-size: 19px">
          {{ detail?.title || detail?.userName || '直播间' }}
          <small v-if="detail && detail.userName">· {{ detail.userName }}</small>
        </h1>
        <div class="sub" style="margin-top: 3px">
          <span v-if="detail" :class="['tag', detail.status ? 'live' : '']" style="margin-right: 8px">
            <i v-if="detail.status" class="status-dot"></i>{{ detail.status ? '直播中' : '未开播' }}
          </span>
          <span v-if="online" class="tag">在线 {{ online }}</span>
        </div>
      </div>
      <div class="spacer"></div>
      <button class="btn sm" @click="addFollow">加入关注</button>
    </div>

    <div v-if="err" class="error" style="margin-bottom: 14px">{{ err }}</div>
    <div v-if="msg" class="tag accent" style="margin-bottom: 14px">{{ msg }}</div>
    <div v-if="loading" class="hint" style="margin-bottom: 14px">加载房间与播放地址中…</div>

    <div class="player-wrap">
      <LivePlayer v-if="line" :key="line.url" :url="line.url" :type="line.type" />
      <DanmakuOverlay
        v-if="playInfo?.danmaku"
        :platform="platform" :room="room" :enabled="danmakuOn"
        @online="onOnline" @log="addLog"
      />
    </div>

    <div class="panel" style="margin-top: 18px">
      <div class="panel-head" style="margin-bottom: 14px">
        <span class="ico">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h10M18 7h2M4 17h2M10 17h10" /><circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" /></svg>
        </span>
        <h2>播放控制</h2>
      </div>
      <div class="row" style="margin-bottom: 12px">
        <strong style="font-size: 13px; min-width: 46px">清晰度</strong>
        <div class="tabs" style="margin: 0; flex: 1">
          <button
            v-for="(q, i) in playInfo?.qualities || []" :key="q.key"
            :class="['btn', 'sm', qi === i ? 'active' : '']"
            @click="setQuality(i)"
          >{{ q.name }}</button>
          <span v-if="!playInfo?.qualities?.length" class="hint">暂无可用清晰度</span>
        </div>
        <button :class="['btn', 'sm', danmakuOn ? '' : 'ghost']" @click="danmakuOn = !danmakuOn">
          {{ danmakuOn ? '弹幕：开' : '弹幕：关' }}
        </button>
        <button class="btn sm ghost" @click="goDanmakuSettings">弹幕设置</button>
      </div>
      <div class="row">
        <strong style="font-size: 13px; min-width: 46px">线路</strong>
        <div class="tabs" style="margin: 0; flex: 1">
          <button
            v-for="(l, i) in quality?.lines || []" :key="i"
            :class="['btn', 'sm', li === i ? 'active' : '']"
            @click="li = i"
          >{{ l.name }} · {{ l.type.toUpperCase() }}</button>
          <span v-if="!quality?.lines?.length" class="hint">暂无可用线路</span>
        </div>
      </div>
      <p class="hint" style="margin: 14px 0 0">
        所有流均经服务端代理转发，自动注入平台所需的 Referer / UA / Cookie，避免浏览器跨域与防盗链问题。HLS 分片与密钥地址会自动重写。
      </p>
    </div>

    <div class="panel" style="margin-top: 18px">
      <div class="panel-head" style="margin-bottom: 10px">
        <span class="ico cyan">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M4 6h16M4 11h10M4 16h13" /></svg>
        </span>
        <h2>弹幕 / 状态</h2>
        <span class="tag">最近 {{ logs.length }} 条</span>
      </div>
      <div class="danmaku-log">
        <div v-for="(l, i) in logs" :key="i">{{ l }}</div>
        <div v-if="!logs.length" class="hint">暂无弹幕消息</div>
      </div>
    </div>
  </div>
</template>
