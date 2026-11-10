<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { api } from './api'

const router = useRouter()
const route = useRoute()
const authEnabled = ref(false)
const authenticated = ref(true)
const ready = ref(false)

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

onMounted(() => {
  check()
  window.addEventListener('silm-unauthorized', onUnauthorized)
})
onBeforeUnmount(() => window.removeEventListener('silm-unauthorized', onUnauthorized))
</script>

<template>
  <div>
    <header class="header" v-if="route.name !== 'login'">
      <div class="header-inner">
        <div class="logo">简<span>播</span></div>
        <nav class="nav">
          <router-link to="/">关注</router-link>
          <router-link to="/search">搜索</router-link>
          <router-link to="/settings">设置</router-link>
        </nav>
        <div class="spacer"></div>
        <span class="tag" v-if="ready">{{ authEnabled ? (authenticated ? '已登录' : '未登录') : '鉴权关闭' }}</span>
      </div>
    </header>
    <router-view v-if="ready || route.name === 'login'" />
  </div>
</template>
