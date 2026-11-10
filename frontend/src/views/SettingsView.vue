<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'
import type { Settings, SiteInfo } from '../types'

const router = useRouter()
const settings = ref<Settings>({ cookies: { douyin: '', kuaishou: '' } })
const sites = ref<SiteInfo[]>([])
const authEnabled = ref(false)
const msg = ref('')
const err = ref('')
const importText = ref('')
const parseText = ref('')

onMounted(async () => {
  try {
    settings.value = await api.getSettings()
    sites.value = (await api.sites()).sites
    authEnabled.value = (await api.authStatus()).enabled
  } catch (e) { err.value = (e as Error).message }
})

async function save() {
  err.value = ''; msg.value = ''
  try { await api.setSettings(settings.value); msg.value = '设置已保存' }
  catch (e) { err.value = (e as Error).message }
}

async function exportJson() {
  err.value = ''
  try {
    const data = await api.exportFollows()
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url; a.download = 'silm-follows.json'; a.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
    msg.value = '已导出关注列表'
  } catch (e) { err.value = (e as Error).message }
}

async function onFile(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  importText.value = await file.text()
}

async function importJson() {
  err.value = ''; msg.value = ''
  try {
    const r = await api.importFollows(JSON.parse(importText.value))
    msg.value = `成功导入 ${r.imported} 条关注`
    importText.value = ''
  } catch (e) { err.value = (e as Error).message }
}

async function doParse() {
  err.value = ''; msg.value = ''
  try {
    const r = await api.parse(parseText.value)
    await router.push({ name: 'player', query: { platform: r.platform, room: r.roomId } })
  } catch (e) { err.value = (e as Error).message }
}
</script>

<template>
  <div class="page">
    <h1 class="page-title">设置 <small>账号 Cookie / 数据导入导出 / 链接解析</small></h1>
    <div v-if="err" class="error" style="margin-bottom: 14px">{{ err }}</div>
    <div v-if="msg" class="tag ok" style="margin-bottom: 14px">{{ msg }}</div>

    <div class="row" style="align-items: flex-start; gap: 16px">
      <div class="panel" style="flex: 1; min-width: 300px">
        <h2 style="margin: 0 0 14px; font-size: 16px">账号 Cookie</h2>
        <p class="hint" style="margin-bottom: 16px">
          抖音搜索 / 弹幕、快手弹幕与搜索需要登录态 Cookie。可直接粘贴整段请求头，服务端会自动提取 cookie 行。
          留空时使用匿名态（播放大多不受影响）。
        </p>
        <div class="field">
          <label>抖音 Cookie</label>
          <textarea v-model="settings.cookies.douyin" placeholder="ttwid=...; sessionid=... 或直接粘贴 Request Headers"></textarea>
        </div>
        <div class="field">
          <label>快手 Cookie</label>
          <textarea v-model="settings.cookies.kuaishou" placeholder="kuaishou.server.web_st=..."></textarea>
        </div>
        <button class="btn primary" @click="save">保存设置</button>
      </div>

      <div style="flex: 1; min-width: 300px; display: flex; flex-direction: column; gap: 16px">
        <div class="panel">
          <h2 style="margin: 0 0 14px; font-size: 16px">关注数据</h2>
          <div class="row" style="margin-bottom: 12px">
            <button class="btn" @click="exportJson">导出 JSON</button>
            <label class="btn" style="cursor: pointer">
              选择文件
              <input type="file" accept=".json,application/json" style="display: none" @change="onFile" />
            </label>
          </div>
          <textarea v-model="importText" placeholder="也可直接粘贴导出的 JSON 内容"></textarea>
          <button class="btn" style="margin-top: 10px" :disabled="!importText.trim()" @click="importJson">导入</button>
        </div>

        <div class="panel">
          <h2 style="margin: 0 0 14px; font-size: 16px">链接解析</h2>
          <p class="hint" style="margin-bottom: 10px">粘贴任意平台分享链接 / 短链，直接进直播间。</p>
          <div class="row">
            <input class="input" v-model="parseText" placeholder="https://live.bilibili.com/..." @keyup.enter="doParse" />
            <button class="btn primary" :disabled="!parseText.trim()" @click="doParse">打开</button>
          </div>
        </div>

        <div class="panel">
          <h2 style="margin: 0 0 14px; font-size: 16px">平台与状态</h2>
          <div class="row" style="gap: 8px">
            <span v-for="s in sites" :key="s.id" class="tag">
              {{ s.name }} · {{ s.searchable ? '可搜索' : '仅链接' }} · {{ s.danmaku ? '弹幕' : '无弹幕' }}
            </span>
          </div>
          <p class="hint" style="margin-top: 12px">
            访问鉴权：{{ authEnabled ? '已开启' : '未开启' }}
            <span v-if="!authEnabled">（在 config.toml 中设置 auth.enabled / auth.password 开启）</span>
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
