<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { api } from './api'

const router = useRouter()
const route = useRoute()
const authEnabled = ref(false)
const authenticated = ref(true)
const ready = ref(false)
const loggingOut = ref(false)

async function check() {
  try {
    const s = await api.authStatus()
    authEnabled.value = s.enabled
    authenticated.value = s.authenticated
    if (s.enabled && !s.authenticated && route.name !== 'login') router.replace('/login')
    if (route.name === 'login' && (!s.enabled || s.authenticated)) router.replace('/')
  } catch {
    // 服务端未启动时保持页面可见，由各页自行报错
  } finally {
    ready.value = true
  }
}

function onUnauthorized() {
  authenticated.value = false
  router.replace('/login')
}

async function doLogout() {
  loggingOut.value = true
  try { await api.logout() } catch { /* ignore */ }
  authenticated.value = false
  loggingOut.value = false
  await check()
}

onMounted(() => {
  check()
  window.addEventListener('silm-unauthorized', onUnauthorized)
  window.addEventListener('silm-auth-changed', check)
})
onBeforeUnmount(() => {
  window.removeEventListener('silm-unauthorized', onUnauthorized)
  window.removeEventListener('silm-auth-changed', check)
})
</script>

<template>
  <div>
    <header class="header" v-if="route.name !== 'login'">
      <div class="header-inner">
        <div class="logo">
          <span class="mark">
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="2.5" y="5" width="19" height="14" rx="3" />
              <path d="M10 9.5l5 2.5-5 2.5z" fill="currentColor" stroke="none" />
            </svg>
          </span>
          简<span class="b">播</span>
        </div>
        <nav class="nav">
          <router-link to="/">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20.5s-7.5-4.6-7.5-10A4.2 4.2 0 0 1 12 7.6a4.2 4.2 0 0 1 7.5 2.9c0 5.4-7.5 10-7.5 10z" /></svg>
            关注
          </router-link>
          <router-link to="/search">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="6.5" /><path d="M16 16l4.5 4.5" /></svg>
            搜索
          </router-link>
          <router-link to="/settings">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h10M18 7h2M4 17h2M10 17h10" /><circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" /></svg>
            设置
          </router-link>
        </nav>
        <div class="spacer"></div>
        <div class="header-actions" v-if="ready">
          <span v-if="!authEnabled" class="tag">公开访问</span>
          <span v-else-if="authenticated" class="tag ok"><i class="status-dot"></i>已验证</span>
          <span v-else class="tag live">未登录</span>
          <button v-if="authEnabled && authenticated" class="btn sm ghost" :disabled="loggingOut" @click="doLogout">退出登录</button>
        </div>
      </div>
    </header>

    <router-view v-if="ready || route.name === 'login'" v-slot="{ Component }">
      <transition name="fade" mode="out-in">
        <component :is="Component" />
      </transition>
    </router-view>

    <div class="footer" v-if="route.name !== 'login'">简播 · 直播聚合面板 — 数据保存在服务端，多设备打开同一地址即可共享</div>
  </div>
</template>
