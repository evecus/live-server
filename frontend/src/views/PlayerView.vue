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
    <div class="row" style="margin-bottom: 14px">
      <button class="btn sm ghost" @click="router.back()">← 返回</button>
      <h1 class="page-title" style="margin: 0; font-size: 18px">
        {{ detail?.title || '直播间' }}
        <small v-if="detail">· {{ detail.userName }}</small>
      </h1>
      <span v-if="detail" :class="['tag', detail.status ? 'live' : '']">{{ detail.status ? '直播中' : '未开播' }}</span>
      <span v-if="online" class="tag">在线 {{ online }}</span>
      <div class="spacer"></div>
      <button class="btn sm" @click="addFollow">加入关注</button>
    </div>

    <div v-if="err" class="error" style="margin-bottom: 14px">{{ err }}</div>
    <div v-if="msg" class="tag" style="margin-bottom: 14px">{{ msg }}</div>
    <div v-if="loading" class="hint" style="margin-bottom: 14px">加载房间与播放地址中…</div>

    <div class="player-wrap">
      <LivePlayer v-if="line" :key="line.url" :url="line.url" :type="line.type" />
      <DanmakuOverlay
        v-if="playInfo?.danmaku"
        :platform="platform" :room="room" :enabled="danmakuOn"
        @online="onOnline" @log="addLog"
      />
    </div>

    <div class="panel" style="margin-top: 16px">
      <div class="row" style="margin-bottom: 10px">
        <strong style="font-size: 13px">清晰度</strong>
        <div class="tabs" style="margin: 0; flex: 1">
          <button
            v-for="(q, i) in playInfo?.qualities || []" :key="q.key"
            :class="['btn', 'sm', qi === i ? 'active' : '']"
            @click="setQuality(i)"
          >{{ q.name }}</button>
        </div>
        <button class="btn sm" @click="danmakuOn = !danmakuOn">
          {{ danmakuOn ? '弹幕：开' : '弹幕：关' }}
        </button>
      </div>
      <div class="row">
        <strong style="font-size: 13px">线路</strong>
        <div class="tabs" style="margin: 0; flex: 1">
          <button
            v-for="(l, i) in quality?.lines || []" :key="i"
            :class="['btn', 'sm', li === i ? 'active' : '']"
            @click="li = i"
          >{{ l.name }} · {{ l.type.toUpperCase() }}</button>
        </div>
      </div>
      <p class="hint" style="margin: 12px 0 0">
        所有流均经服务端代理转发，自动注入平台所需的 Referer / UA / Cookie，避免浏览器跨域与防盗链问题。
        HLS 分片与密钥地址会自动重写。
      </p>
    </div>

    <div class="panel" style="margin-top: 16px">
      <div class="row" style="margin-bottom: 8px">
        <strong style="font-size: 13px">弹幕 / 状态</strong>
        <span class="muted" style="font-size: 12px">最近 {{ logs.length }} 条</span>
      </div>
      <div class="danmaku-log">
        <div v-for="(l, i) in logs" :key="i">{{ l }}</div>
        <div v-if="!logs.length" class="hint">暂无弹幕消息</div>
      </div>
    </div>
  </div>
</template>
