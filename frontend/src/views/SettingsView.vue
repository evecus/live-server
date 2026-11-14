<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { api } from '../api'
import { DanmakuRenderer, SAMPLE_DANMAKU } from '../danmakuRenderer'
import { defaultDanmaku, defaultSettings, type DanmakuColorMode, type Settings, type SiteInfo } from '../types'

const route = useRoute()
const router = useRouter()
const settings = ref<Settings>(defaultSettings())
const sites = ref<SiteInfo[]>([])
/** 服务端实际鉴权状态；验证面板的表单单独管理，避免误提交 */
const authEnabled = ref(false)
const authForm = ref({ enabled: false, password: '', hasPassword: false })
const msg = ref('')
const err = ref('')
const loaded = ref(false)
const saving = ref(false)
const savingAuth = ref(false)
const importText = ref('')
const parseText = ref('')
const activeSection = ref('danmaku')

const sections = [
  { id: 'danmaku', label: '弹幕显示' },
  { id: 'auth', label: '访问验证' },
  { id: 'cookies', label: '账号 Cookie' },
  { id: 'data', label: '关注数据' },
  { id: 'parse', label: '链接解析' },
  { id: 'about', label: '平台与状态' },
]
const colorModes: Array<{ id: DanmakuColorMode; label: string }> = [
  { id: 'platform', label: '跟随平台' },
  { id: 'custom', label: '自定义' },
  { id: 'white', label: '纯白' },
  { id: 'rainbow', label: '缤纷' },
]

const previewCanvas = ref<HTMLCanvasElement | null>(null)
let renderer: DanmakuRenderer | null = null
let sampleTimer = 0
let sampleIndex = 0

function pct(v: number, min: number, max: number) {
  return `${Math.max(0, Math.min(100, ((v - min) / (max - min)) * 100))}%`
}

function feed() {
  const s = settings.value.danmaku
  const item = SAMPLE_DANMAKU[sampleIndex++ % SAMPLE_DANMAKU.length]
  const text = s.showUser && item.user ? `${item.user}：${item.text}` : item.text
  renderer?.push(text, item.color)
}

function startPreview() {
  renderer?.clear()
  sampleIndex = 0
  for (let i = 0; i < 4; i++) feed()
  renderer?.scatter()
  clearInterval(sampleTimer)
  sampleTimer = window.setInterval(feed, 900)
}

function resetDanmaku() {
  settings.value.danmaku = defaultDanmaku()
  renderer?.setSettings(settings.value.danmaku)
  startPreview()
  msg.value = '弹幕参数已恢复默认，记得保存'
}

onMounted(async () => {
  try {
    settings.value = await api.getSettings()
    sites.value = (await api.sites()).sites
    authEnabled.value = (await api.authStatus()).enabled
    authForm.value = {
      enabled: settings.value.panelAuth.enabled,
      password: '',
      hasPassword: !!settings.value.panelAuth.hasPassword,
    }
  } catch (e) { err.value = (e as Error).message }
  // 数据到位后才渲染控件，避免异步返回覆盖用户刚调整的参数
  loaded.value = true
  await nextTick()
  if (previewCanvas.value) {
    renderer = new DanmakuRenderer(previewCanvas.value, settings.value.danmaku)
    renderer.start()
    startPreview()
  }
  const section = String(route.query.section || '')
  if (section) setTimeout(() => go(section, true), 320)
})

watch(() => settings.value.danmaku, (v) => { renderer?.setSettings(v) }, { deep: true })
onBeforeUnmount(() => { clearInterval(sampleTimer); renderer?.stop(); renderer = null })

function topOf(id: string): number | null {
  const el = document.getElementById(id)
  return el ? el.getBoundingClientRect().top + window.scrollY - 82 : null
}

function go(id: string, instant = false) {
  activeSection.value = id
  const scroll = () => {
    const top = topOf(id)
    if (top !== null) window.scrollTo(0, top)
  }
  if (!instant) {
    const top = topOf(id)
    if (top !== null) window.scrollTo({ top, behavior: 'smooth' })
    return
  }
  // 路由过渡期间布局尚未稳定，稍后再校正一次，避免滚动跳动
  scroll()
  requestAnimationFrame(() => setTimeout(scroll, 260))
}

/** 通用保存：面板验证状态沿用服务端实际值，只有「保存验证设置」才会改动验证 */
async function saveAll() {
  err.value = ''; msg.value = ''; saving.value = true
  try {
    await api.setSettings({
      ...settings.value,
      panelAuth: { enabled: authEnabled.value, password: '' },
    })
    window.dispatchEvent(new CustomEvent('silm-settings-changed'))
    msg.value = '设置已保存，弹幕参数即时生效'
  } catch (e) { err.value = (e as Error).message }
  finally { saving.value = false }
}

async function saveAuth() {
  err.value = ''; msg.value = ''; savingAuth.value = true
  const pw = authForm.value.password.trim()
  try {
    await api.setSettings({
      ...settings.value,
      panelAuth: { enabled: authForm.value.enabled, password: pw },
    })
    authEnabled.value = authForm.value.enabled
    authForm.value.hasPassword = authForm.value.hasPassword || !!pw
    authForm.value.password = ''
    if (authForm.value.enabled && pw) {
      try { await api.login(pw) } catch { /* 交由状态检查决定是否跳登录页 */ }
    }
    window.dispatchEvent(new CustomEvent('silm-settings-changed'))
    window.dispatchEvent(new CustomEvent('silm-auth-changed'))
    if (!authForm.value.enabled) msg.value = '访问验证已关闭，面板恢复公开访问'
    else if (pw) msg.value = '访问验证已开启，本设备已保持登录'
    else msg.value = '访问验证已开启，页面将跳转登录页，请输入访问密码'
  } catch (e) { err.value = (e as Error).message }
  finally { savingAuth.value = false }
}

async function doLogout() {
  try { await api.logout() } catch { /* ignore */ }
  window.dispatchEvent(new CustomEvent('silm-auth-changed'))
  msg.value = '已退出登录'
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
    <div class="page-head">
      <div>
        <h1 class="page-title">设置</h1>
        <div class="sub">弹幕外观、面板访问验证与数据管理 · 保存后对所有设备生效</div>
      </div>
      <div class="spacer"></div>
      <button class="btn primary" :disabled="saving" @click="saveAll">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" /><path d="M17 21v-8H7v8M7 3v5h8" /></svg>
        {{ saving ? '保存中…' : '保存全部设置' }}
      </button>
    </div>

    <div v-if="err" class="error" style="margin-bottom: 14px">{{ err }}</div>
    <div v-if="msg" class="tag ok" style="margin-bottom: 14px">{{ msg }}</div>

    <div v-if="loaded" class="settings-layout">
      <aside class="settings-nav">
        <button v-for="s in sections" :key="s.id" class="item" :class="{ active: activeSection === s.id }" @click="go(s.id)">
          <i class="dot"></i>{{ s.label }}
        </button>
      </aside>

      <div class="settings-main">
        <!-- 弹幕显示 -->
        <section id="danmaku" class="panel">
          <div class="panel-head">
            <span class="ico cyan">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h16M4 11h10M4 16h13" /><circle cx="18.5" cy="11" r="1.4" fill="currentColor" stroke="none" /></svg>
            </span>
            <h2>弹幕显示</h2>
            <span class="tag accent">实时预览</span>
          </div>
          <p class="panel-desc">字号、屏占比、间距、颜色、速度等参数即时作用于播放器弹幕层；保存后写入服务端，其它设备打开同一地址即可共享。</p>

          <div class="preview-stage">
            <canvas ref="previewCanvas"></canvas>
            <span class="stage-label">简播 · 弹幕效果实时预览</span>
          </div>
          <div class="preview-toolbar">
            <span class="tag live"><i class="status-dot"></i>实时滚动</span>
            <button class="btn sm" @click="startPreview">重播预览</button>
            <button class="btn sm ghost" @click="resetDanmaku">恢复默认</button>
            <span class="hint">示例弹幕持续滚动，调整下方参数即可看到变化</span>
          </div>

          <hr class="panel-divider" />
          <div class="setting-grid">
            <div class="setting-item">
              <div class="setting-head"><span class="name">启用弹幕</span></div>
              <label class="switch">
                <input type="checkbox" v-model="settings.danmaku.enabled" />
                <span class="track"></span><span class="label">在播放器中显示弹幕</span>
              </label>
              <div class="setting-note">关闭后播放器不再渲染弹幕，仍保留弹幕日志</div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">字号</span><span class="val">{{ settings.danmaku.fontSize }} px</span></div>
              <input class="range" type="range" min="12" max="42" step="1" v-model.number="settings.danmaku.fontSize" :style="{ '--p': pct(settings.danmaku.fontSize, 12, 42) }" />
              <div class="setting-note">弹幕文字大小，默认 22 px</div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">屏占比</span><span class="val">{{ settings.danmaku.area }}%</span></div>
              <input class="range" type="range" min="20" max="100" step="5" v-model.number="settings.danmaku.area" :style="{ '--p': pct(settings.danmaku.area, 20, 100) }" />
              <div class="setting-note">弹幕可使用的画面高度比例，越小越不挡画面</div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">弹幕间距（行）</span><span class="val">{{ settings.danmaku.lineGap }} px</span></div>
              <input class="range" type="range" min="0" max="32" step="1" v-model.number="settings.danmaku.lineGap" :style="{ '--p': pct(settings.danmaku.lineGap, 0, 32) }" />
              <div class="setting-note">上下相邻两条弹幕轨道之间的空隙</div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">弹幕间距（水平）</span><span class="val">{{ settings.danmaku.itemGap }} px</span></div>
              <input class="range" type="range" min="20" max="200" step="4" v-model.number="settings.danmaku.itemGap" :style="{ '--p': pct(settings.danmaku.itemGap, 20, 200) }" />
              <div class="setting-note">同一轨道前后两条弹幕的最小间隔，避免重叠</div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">滚动速度</span><span class="val">{{ settings.danmaku.duration }} 秒 / 屏</span></div>
              <input class="range" type="range" min="3" max="20" step="0.5" v-model.number="settings.danmaku.duration" :style="{ '--p': pct(settings.danmaku.duration, 3, 20) }" />
              <div class="setting-note">弹幕横穿整个画面所需时间，数值越小越快</div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">不透明度</span><span class="val">{{ Math.round(settings.danmaku.opacity * 100) }}%</span></div>
              <input class="range" type="range" min="0.2" max="1" step="0.05" v-model.number="settings.danmaku.opacity" :style="{ '--p': pct(settings.danmaku.opacity, 0.2, 1) }" />
              <div class="setting-note">降低透明度可让弹幕更柔和</div>
            </div>
          </div>

          <hr class="panel-divider" />
          <div class="setting-grid">
            <div class="setting-item" style="grid-column: 1 / -1">
              <div class="setting-head"><span class="name">弹幕颜色</span><span class="val">{{ colorModes.find((m) => m.id === settings.danmaku.colorMode)?.label }}</span></div>
              <div class="seg">
                <button v-for="m in colorModes" :key="m.id" :class="{ active: settings.danmaku.colorMode === m.id }" @click="settings.danmaku.colorMode = m.id">{{ m.label }}</button>
              </div>
              <div v-if="settings.danmaku.colorMode === 'custom'" class="row" style="margin-top: 12px">
                <input type="color" class="color-input" v-model="settings.danmaku.color" />
                <input class="input" style="max-width: 160px" v-model="settings.danmaku.color" placeholder="#ffffff" />
                <span class="hint">所有弹幕统一使用该颜色</span>
              </div>
              <div class="setting-note">跟随平台：使用平台下发的颜色 · 缤纷：自动分配柔和色板 · 纯白：统一白色</div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">文字样式</span></div>
              <div style="display: flex; flex-direction: column; gap: 12px; margin-top: 2px">
                <label class="switch"><input type="checkbox" v-model="settings.danmaku.bold" /><span class="track"></span><span class="label">文字加粗</span></label>
                <label class="switch"><input type="checkbox" v-model="settings.danmaku.stroke" /><span class="track"></span><span class="label">描边阴影（复杂画面更清晰）</span></label>
                <label class="switch"><input type="checkbox" v-model="settings.danmaku.showUser" /><span class="track"></span><span class="label">显示用户名前缀</span></label>
              </div>
            </div>
            <div class="setting-item">
              <div class="setting-head"><span class="name">轨道分配</span></div>
              <div class="notice">
                弹幕会按 <strong>屏占比</strong> 与 <strong>行间距</strong> 自动计算可用轨道，并优先选择最空闲的一条；同轨道需满足
                <strong>水平间距</strong> 才会放入下一条，拥挤时自动排队，不会互相重叠。
              </div>
            </div>
          </div>

          <div class="row" style="margin-top: 20px">
            <button class="btn primary" :disabled="saving" @click="saveAll">保存弹幕设置</button>
            <span class="hint">预览已即时生效，保存后其它设备刷新即可同步</span>
          </div>
        </section>

        <!-- 面板访问验证 -->
        <section id="auth" class="panel">
          <div class="panel-head">
            <span class="ico">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="10" width="16" height="11" rx="2.5" /><path d="M8 10V7a4 4 0 0 1 8 0v3" /></svg>
            </span>
            <h2>面板访问验证</h2>
            <span :class="['tag', authEnabled ? 'ok' : '']">{{ authEnabled ? '已开启' : '未开启' }}</span>
          </div>
          <p class="panel-desc">按需开启密码保护。开启后所有接口与弹幕连接都需要登录，未登录的设备会自动跳转登录页；关闭则同一网络内可直接访问。</p>

          <div class="auth-banner" :class="{ on: authForm.enabled }">
            <span class="ico">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="10" width="16" height="11" rx="2.5" /><path d="M8 10V7a4 4 0 0 1 8 0v3" /></svg>
            </span>
            <div>
              <div class="t">{{ authForm.enabled ? '访问验证已开启（待保存）' : '当前为公开访问' }}</div>
              <div class="d">{{ authForm.enabled ? '保存后所有 API / WS 均需密码登录' : '局域网内任何设备都可直接打开面板' }}</div>
            </div>
            <div class="spacer"></div>
            <label class="switch"><input type="checkbox" v-model="authForm.enabled" /><span class="track"></span></label>
          </div>

          <div class="setting-grid">
            <div class="setting-item">
              <label>访问密码</label>
              <div class="password-wrap">
                <input class="input" type="password" v-model="authForm.password" autocomplete="new-password"
                  :placeholder="authForm.hasPassword ? '已设置，留空表示不修改' : '请设置访问密码'" />
              </div>
              <div class="field-hint">密码仅保存在服务端，不会回显到页面。留空表示沿用当前密码；修改后其它设备需重新登录。</div>
            </div>
            <div class="setting-item">
              <label>当前状态</label>
              <div class="row" style="gap: 8px">
                <span :class="['tag', authEnabled ? 'ok' : 'accent']"><i v-if="authEnabled" class="status-dot"></i>{{ authEnabled ? '面板已保护' : '公开访问' }}</span>
                <span v-if="authForm.hasPassword" class="tag">密码已设置</span>
              </div>
              <div class="field-hint">也可通过 config.toml 的 auth.enabled / auth.password，或命令行参数设置初始状态；面板设置优先并持久化。</div>
            </div>
          </div>

          <div class="notice" style="margin-top: 16px">
            <strong>小提示：</strong>填写新密码并保存时，本设备会自动完成登录，不会被锁在面板外；若沿用已有密码且当前未登录，保存后会跳转登录页输入密码。
          </div>
          <div class="row" style="margin-top: 16px">
            <button class="btn primary" :disabled="savingAuth" @click="saveAuth">{{ savingAuth ? '保存中…' : '保存验证设置' }}</button>
            <button v-if="authEnabled" class="btn" @click="doLogout">退出登录</button>
          </div>
        </section>

        <!-- 账号 Cookie -->
        <section id="cookies" class="panel">
          <div class="panel-head">
            <span class="ico amber">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>
            </span>
            <h2>账号 Cookie</h2>
          </div>
          <p class="panel-desc">抖音搜索 / 弹幕、快手弹幕与搜索需要登录态 Cookie。可直接粘贴整段请求头，服务端会自动提取 cookie 行；留空时使用匿名态。</p>
          <div class="field">
            <label>抖音 Cookie</label>
            <textarea v-model="settings.cookies.douyin" placeholder="ttwid=...; sessionid=... 或直接粘贴 Request Headers"></textarea>
          </div>
          <div class="field">
            <label>快手 Cookie</label>
            <textarea v-model="settings.cookies.kuaishou" placeholder="kuaishou.server.web_st=..."></textarea>
          </div>
          <button class="btn" :disabled="saving" @click="saveAll">保存 Cookie</button>
        </section>

        <!-- 关注数据 -->
        <section id="data" class="panel">
          <div class="panel-head">
            <span class="ico green">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12m0 0l-4-4m4 4l4-4" /><path d="M4 17v3h16v-3" /></svg>
            </span>
            <h2>关注数据</h2>
          </div>
          <p class="panel-desc">导出为 JSON 备份，或从安卓端 / 其它设备导入关注列表。</p>
          <div class="row" style="margin-bottom: 12px">
            <button class="btn" @click="exportJson">导出 JSON</button>
            <label class="btn" style="cursor: pointer">
              选择文件
              <input type="file" accept=".json,application/json" style="display: none" @change="onFile" />
            </label>
          </div>
          <textarea v-model="importText" placeholder="也可直接粘贴导出的 JSON 内容"></textarea>
          <button class="btn" style="margin-top: 10px" :disabled="!importText.trim()" @click="importJson">导入</button>
        </section>

        <!-- 链接解析 -->
        <section id="parse" class="panel">
          <div class="panel-head">
            <span class="ico rose">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.5.5l2-2a5 5 0 0 0-7-7l-1.2 1.2" /><path d="M14 11a5 5 0 0 0-7.5-.5l-2 2a5 5 0 0 0 7 7l1.2-1.2" /></svg>
            </span>
            <h2>链接解析</h2>
          </div>
          <p class="panel-desc">粘贴任意平台分享链接 / 短链，直接进直播间。</p>
          <div class="row">
            <input class="input" v-model="parseText" placeholder="https://live.bilibili.com/..." @keyup.enter="doParse" />
            <button class="btn primary" :disabled="!parseText.trim()" @click="doParse">打开</button>
          </div>
        </section>

        <!-- 平台与状态 -->
        <section id="about" class="panel">
          <div class="panel-head">
            <span class="ico">
              <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a15 15 0 0 1 0 18M12 3a15 15 0 0 0 0 18" /></svg>
            </span>
            <h2>平台与状态</h2>
          </div>
          <div class="site-tags">
            <span v-for="s in sites" :key="s.id" class="site-tag">
              <i class="dot"></i>{{ s.name }}
              <small>{{ s.searchable ? '可搜索' : '仅链接' }} · {{ s.danmaku ? '弹幕' : '无弹幕' }}</small>
            </span>
            <span v-if="!sites.length" class="hint">正在读取平台信息…</span>
          </div>
          <p class="hint" style="margin-top: 14px">
            所有流均经服务端代理转发并注入平台所需的 Referer / UA / Cookie，前端为纯静态页面，编译进二进制后单文件即可运行。
          </p>
        </section>
      </div>
    </div>
    <div v-else class="panel hint" style="text-align: center; padding: 44px">正在读取设置…</div>
  </div>
</template>
