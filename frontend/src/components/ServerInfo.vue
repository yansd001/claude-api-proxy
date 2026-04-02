<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { api } from '../api'
import type { AppConfig, ServerConfig } from '../types'

const props = defineProps<{ config: AppConfig }>()
const emit = defineEmits<{ updated: [] }>()

const form = ref<ServerConfig>({ ...props.config.server })
const saving = ref(false)
const newKeyVisible = ref(false)
const isDocker = ref(false)

const proxyBaseUrl = computed(() => {
  const host = form.value.host === '0.0.0.0' ? 'localhost' : form.value.host
  return `http://${host}:${form.value.port}`
})

const settingsJson = computed(() => {
  return JSON.stringify({
    env: {
      ANTHROPIC_AUTH_TOKEN: form.value.api_key,
      ANTHROPIC_BASE_URL: proxyBaseUrl.value,
      API_TIMEOUT_MS: "300000"
    }
  }, null, 2)
})

const linuxWriteCommand = computed(() => {
  const json = settingsJson.value.replace(/'/g, "'\\''")
  return `mkdir -p ~/.claude && cat > ~/.claude/settings.json << 'EOF'\n${json}\nEOF`
})

const windowsWriteCommand = computed(() => {
  const escaped = settingsJson.value.replace(/"/g, '\\"')
  return `if not exist "%USERPROFILE%\\.claude" mkdir "%USERPROFILE%\\.claude"\necho ${escaped} > "%USERPROFILE%\\.claude\\settings.json"`
})

async function save() {
  saving.value = true
  try {
    await api.updateServer(form.value)
    ElMessage.success('服务器配置已保存（需重启 backend 使端口/host 变更生效）')
    emit('updated')
  } catch {
    ElMessage.error('保存失败')
  } finally {
    saving.value = false
  }
}

function regenerateKey() {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
  form.value.api_key = Array.from(
    { length: 40 },
    () => chars[Math.floor(Math.random() * chars.length)]
  ).join('')
}

function copyText(text: string) {
  navigator.clipboard.writeText(text).then(() => ElMessage.success('已复制'))
}

// ---- Claude Code management (Windows exe mode only) ----
const claudeInstalled = ref<boolean | null>(null)
const claudeChecking = ref(false)
const installing = ref(false)
const configuringProxy = ref(false)
const externalDialogVisible = ref(false)
const configuringExternal = ref(false)
const externalForm = ref({ base_url: 'https://yansd666.com', api_key: '' })
const activeTab = ref('linux')

async function checkClaudeInstalled() {
  claudeChecking.value = true
  try {
    const status = await api.claudeCodeStatus()
    claudeInstalled.value = status.installed
  } catch {
    claudeInstalled.value = null
  } finally {
    claudeChecking.value = false
  }
}

async function installClaude() {
  installing.value = true
  try {
    await api.installClaudeCode()
    ElMessage.success('Claude Code 安装成功！')
    claudeInstalled.value = true
  } catch (e: any) {
    ElMessage.error(e?.response?.data?.detail || '安装失败，请手动运行：npm install -g @anthropic-ai/claude-code')
  } finally {
    installing.value = false
  }
}

async function configureProxyToClaude() {
  configuringProxy.value = true
  try {
    await api.configureClaudeProxy()
    ElMessage.success('已成功写入 ~/.claude/settings.json（代理接入配置）')
  } catch (e: any) {
    ElMessage.error(e?.response?.data?.detail || '配置失败')
  } finally {
    configuringProxy.value = false
  }
}

async function configureExternalToClaude() {
  if (!externalForm.value.api_key) {
    ElMessage.warning('请输入 API Key')
    return
  }
  configuringExternal.value = true
  try {
    await api.configureClaudeExternal(externalForm.value.base_url, externalForm.value.api_key)
    ElMessage.success('已成功写入 ~/.claude/settings.json（中转站 API 配置）')
    externalDialogVisible.value = false
    externalForm.value = { base_url: 'https://yansd666.com', api_key: '' }
  } catch (e: any) {
    ElMessage.error(e?.response?.data?.detail || '配置失败')
  } finally {
    configuringExternal.value = false
  }
}

onMounted(async () => {
  try {
    const info = await api.getRuntimeInfo()
    isDocker.value = info.docker
  } catch {
    isDocker.value = false
  }
  if (!isDocker.value) {
    checkClaudeInstalled()
  }
})
</script>

<template>
  <div class="section">
    <h3 class="section-title">在 Claude Code 中使用以下信息</h3>

    <el-descriptions :column="1" border class="proxy-info">
      <el-descriptions-item label="ANTHROPIC_BASE_URL">
        <span class="mono">{{ proxyBaseUrl }}</span>
        <el-button
          size="small"
          text
          :icon="'CopyDocument'"
          @click="copyText(proxyBaseUrl)"
          class="copy-btn"
        />
      </el-descriptions-item>
      <el-descriptions-item label="ANTHROPIC_API_KEY">
        <span class="mono">{{ newKeyVisible ? form.api_key : '•'.repeat(24) }}</span>
        <el-button
          size="small"
          text
          @click="newKeyVisible = !newKeyVisible"
          class="copy-btn"
        >{{ newKeyVisible ? '隐藏' : '显示' }}</el-button>
        <el-button
          size="small"
          text
          :icon="'CopyDocument'"
          @click="copyText(form.api_key)"
          class="copy-btn"
        />
      </el-descriptions-item>
    </el-descriptions>

    <el-alert type="info" :closable="false" style="margin-top:16px">
      <template #default>
        启动 Claude Code 命令示例：
        <br />
        <code>ANTHROPIC_BASE_URL={{ proxyBaseUrl }} ANTHROPIC_API_KEY={{ form.api_key }} claude</code>
      </template>
    </el-alert>

    <el-divider />
    <h3 class="section-title">Claude Code 快捷操作</h3>

    <!-- ==================== Docker mode ==================== -->
    <template v-if="isDocker">
      <div class="hint" style="margin-bottom:12px">
        当前运行在 Docker 容器中，请在运行 Claude Code 的机器上手动配置 <code>settings.json</code>
      </div>

      <h4 class="sub-title">配置文件内容</h4>
      <div class="code-block-wrapper">
        <pre class="code-block">{{ settingsJson }}</pre>
        <el-button
          size="small"
          type="primary"
          class="code-copy-btn"
          @click="copyText(settingsJson)"
        >复制内容</el-button>
      </div>

      <el-tabs v-model="activeTab" style="margin-top:16px">
        <el-tab-pane label="Linux / macOS" name="linux">
          <div class="path-info">
            <span class="path-label">配置文件路径：</span>
            <code>~/.claude/settings.json</code>
          </div>
          <h4 class="sub-title">一键写入命令</h4>
          <div class="code-block-wrapper">
            <pre class="code-block">{{ linuxWriteCommand }}</pre>
            <el-button
              size="small"
              type="primary"
              class="code-copy-btn"
              @click="copyText(linuxWriteCommand)"
            >复制命令</el-button>
          </div>
        </el-tab-pane>
        <el-tab-pane label="Windows" name="windows">
          <div class="path-info">
            <span class="path-label">配置文件路径：</span>
            <code>%USERPROFILE%\.claude\settings.json</code>
          </div>
          <h4 class="sub-title">PowerShell 一键写入命令</h4>
          <div class="code-block-wrapper">
            <pre class="code-block">{{ windowsWriteCommand }}</pre>
            <el-button
              size="small"
              type="primary"
              class="code-copy-btn"
              @click="copyText(windowsWriteCommand)"
            >复制命令</el-button>
          </div>
        </el-tab-pane>
      </el-tabs>

      <div class="hint" style="margin-top:8px">
        将以上内容写入对应路径的配置文件后，重启 Claude Code 即可生效
      </div>
    </template>

    <!-- ==================== Windows exe mode ==================== -->
    <template v-else>
      <div class="action-row">
        <el-button
          type="success"
          :loading="configuringProxy"
          @click="configureProxyToClaude"
        >
          一键配置代理到 Claude Code
        </el-button>
        <el-button
          type="warning"
          @click="externalDialogVisible = true"
        >
          一键配置中转站 API 到 Claude Code
        </el-button>
      </div>
      <div class="hint">以上操作将接入信息写入 ~/.claude/settings.json，Claude Code 重启后生效</div>

      <div class="install-section">
        <div v-if="claudeChecking" class="install-row">
          <el-text type="info">正在检测 Claude Code 安装状态...</el-text>
        </div>
        <div v-else-if="claudeInstalled === true" class="install-row">
          <el-tag type="success" size="large">✓ Claude Code 已安装</el-tag>
          <el-button size="small" text @click="checkClaudeInstalled">重新检测</el-button>
        </div>
        <div v-else class="install-row">
          <el-tag type="danger" size="large">✗ Claude Code 未安装</el-tag>
          <el-button
            type="primary"
            :loading="installing"
            @click="installClaude"
            style="margin-left:12px"
          >
            一键安装 Claude Code
          </el-button>
          <el-button size="small" text @click="checkClaudeInstalled" style="margin-left:4px">重新检测</el-button>
          <div class="hint" style="margin-top:4px">将运行：npm install -g @anthropic-ai/claude-code</div>
        </div>
      </div>

      <!-- External API Dialog -->
      <el-dialog
        v-model="externalDialogVisible"
        title="配置中转站 API 到 Claude Code"
        width="460px"
        destroy-on-close
      >
        <el-form :model="externalForm" label-width="90px">
          <el-form-item label="Base URL">
            <el-input v-model="externalForm.base_url" placeholder="https://yansd666.com" />
          </el-form-item>
          <el-form-item label="API Key" required>
            <el-input
              v-model="externalForm.api_key"
              placeholder="请输入中转站 API Key"
              show-password
            />
          </el-form-item>
        </el-form>
        <el-alert type="info" :closable="false" style="margin: 0 20px 12px; font-size:12px">
          将写入 ~/.claude/settings.json 的 env.ANTHROPIC_AUTH_TOKEN 和 env.ANTHROPIC_BASE_URL，不影响其他配置项
        </el-alert>
        <template #footer>
          <el-button @click="externalDialogVisible = false">取消</el-button>
          <el-button
            type="primary"
            :loading="configuringExternal"
            @click="configureExternalToClaude"
          >确定</el-button>
        </template>
      </el-dialog>
    </template>

    <el-divider />
    <h3 class="section-title">服务器设置</h3>

    <el-form :model="form" label-width="120px" style="max-width:560px">
      <el-form-item label="监听 Host">
        <el-input v-model="form.host" placeholder="0.0.0.0" />
        <div class="hint">0.0.0.0 允许外部访问；127.0.0.1 仅本机</div>
      </el-form-item>
      <el-form-item label="端口">
        <el-input-number v-model="form.port" :min="1024" :max="65535" />
      </el-form-item>
      <el-form-item label="代理 API Key">
        <el-input v-model="form.api_key" show-password />
        <el-button size="small" style="margin-top:6px" @click="regenerateKey">
          随机生成新 Key
        </el-button>
      </el-form-item>
      <el-form-item>
        <el-button type="primary" :loading="saving" @click="save">保存服务器配置</el-button>
      </el-form-item>
    </el-form>
  </div>
</template>

<style scoped>
.section-title { margin: 0 0 16px; font-size: 16px; font-weight: 600; }
.sub-title { margin: 12px 0 8px; font-size: 14px; font-weight: 600; color: #333; }
.proxy-info { margin-bottom: 8px; }
.mono { font-family: monospace; font-size: 13px; }
.copy-btn { margin-left: 8px; }
.hint { color: #999; font-size: 12px; margin-top: 4px; }
.action-row { display: flex; gap: 12px; flex-wrap: wrap; margin-bottom: 8px; }
.install-section { margin-top: 16px; }
.install-row { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; }
.path-info { margin: 8px 0; font-size: 13px; }
.path-label { color: #666; }
.path-info code { background: #f5f5f5; padding: 2px 6px; border-radius: 3px; font-size: 13px; }
.code-block-wrapper { position: relative; }
.code-block {
  background: #1e1e1e;
  color: #d4d4d4;
  padding: 14px 16px;
  border-radius: 6px;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  font-size: 13px;
  line-height: 1.5;
  overflow-x: auto;
  white-space: pre;
  margin: 0;
}
.code-copy-btn { position: absolute; top: 8px; right: 8px; }
</style>
