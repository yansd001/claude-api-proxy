<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api } from './api'
import type { AppConfig } from './types'
import ServerInfo from './components/ServerInfo.vue'
import ProviderList from './components/ProviderList.vue'

const config = ref<AppConfig | null>(null)
const loading = ref(true)

async function loadConfig() {
  try {
    config.value = await api.getConfig()
  } catch {
    ElMessage.error('无法连接到后端服务，请确认 backend 已启动（python main.py）')
  } finally {
    loading.value = false
  }
}

onMounted(loadConfig)
</script>

<template>
  <div class="app-wrapper">
    <header class="app-header">
      <h1 class="title">Claude API 代理配置</h1>
      <span class="subtitle">将 Claude Code 请求转发到 OpenAI / Gemini / Anthropic</span>
    </header>

    <el-container v-if="!loading && config" class="main-content">
      <el-tabs type="border-card" style="width:100%">
        <!-- Tab 1: Server Info -->
        <el-tab-pane>
          <template #label>
            <el-icon><Monitor /></el-icon> 接入信息
          </template>
          <ServerInfo :config="config" @updated="loadConfig" />
        </el-tab-pane>

        <!-- Tab 2: Providers -->
        <el-tab-pane>
          <template #label>
            <el-icon><Connection /></el-icon> 提供商
          </template>
          <ProviderList :config="config" @updated="loadConfig" />
        </el-tab-pane>
      </el-tabs>
    </el-container>

    <div v-else-if="loading" class="loading-placeholder">
      <el-skeleton :rows="8" animated />
    </div>
  </div>
</template>

<style>
* { box-sizing: border-box; }
body {
  margin: 0;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  background: #f0f2f5;
}
.app-wrapper {
  max-width: 1080px;
  margin: 0 auto;
  padding: 24px 16px 48px;
}
.app-header {
  margin-bottom: 24px;
}
.title {
  margin: 0 0 4px;
  font-size: 24px;
  font-weight: 700;
  color: #1a1a2e;
}
.subtitle {
  color: #666;
  font-size: 14px;
}
.main-content { width: 100%; }
.loading-placeholder { padding: 32px; }
</style>
