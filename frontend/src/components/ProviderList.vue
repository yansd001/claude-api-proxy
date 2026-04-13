<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api } from '../api'
import type { AppConfig, Provider, ProviderType } from '../types'

const props = defineProps<{ config: AppConfig }>()
const emit = defineEmits<{ updated: [] }>()

// ---- Dialog state ----
const dialogVisible = ref(false)
const isEdit = ref(false)
const editingId = ref('')
const apiKeyVisible = ref(false)
const fetchingModels = ref(false)
const remoteModels = ref<string[]>([])   // models fetched from remote API

const emptyForm = (): Omit<Provider, 'id'> => ({
  type: 'openai',
  name: '',
  base_url: '',
  api_key: '',
  models: [],
  enabled: true,
})

const form = ref(emptyForm())

const defaultBaseUrls: Record<ProviderType, string> = {
  openai: 'https://yansd666.com',
  gemini: 'https://generativelanguage.googleapis.com',
}

function onTypeChange(type: ProviderType) {
  if (!form.value.base_url || Object.values(defaultBaseUrls).includes(form.value.base_url)) {
    form.value.base_url = defaultBaseUrls[type]
  }
}

function openAdd() {
  isEdit.value = false
  form.value = emptyForm()
  form.value.base_url = defaultBaseUrls['openai']
  remoteModels.value = []
  apiKeyVisible.value = false
  dialogVisible.value = true
}

function openEdit(p: Provider) {
  isEdit.value = true
  editingId.value = p.id
  form.value = { ...p, models: [...p.models] }
  remoteModels.value = [...p.models]
  apiKeyVisible.value = false
  dialogVisible.value = true
}

async function fetchRemoteModels() {
  if (!form.value.base_url || !form.value.api_key) {
    ElMessage.warning('请先填写 Base URL 和 API Key')
    return
  }
  fetchingModels.value = true
  try {
    const result = await api.fetchModels(form.value.base_url, form.value.api_key)
    remoteModels.value = result.models
    ElMessage.success(`获取到 ${result.models.length} 个模型`)
  } catch (e: any) {
    ElMessage.error(e?.response?.data?.detail || '获取模型列表失败，请检查 Base URL 和 API Key')
  } finally {
    fetchingModels.value = false
  }
}

async function submitForm() {
  if (!form.value.name || !form.value.base_url || !form.value.api_key) {
    ElMessage.warning('请填写所有必填字段')
    return
  }

  try {
    if (isEdit.value) {
      await api.updateProvider(editingId.value, { id: editingId.value, ...form.value })
      ElMessage.success('提供商已更新')
    } else {
      await api.addProvider(form.value)
      ElMessage.success('提供商已添加')
    }
    dialogVisible.value = false
    emit('updated')
  } catch {
    ElMessage.error('操作失败')
  }
}

async function removeProvider(id: string) {
  try {
    await ElMessageBox.confirm('确定删除该提供商？相关模型映射也会被删除。', '确认删除', {
      type: 'warning',
    })
    await api.deleteProvider(id)
    ElMessage.success('已删除')
    emit('updated')
  } catch {
    // user cancelled
  }
}

async function toggleEnabled(p: Provider) {
  try {
    await api.updateProvider(p.id, p)
    emit('updated')
  } catch {
    ElMessage.error('操作失败')
  }
}

function typeTag(type: ProviderType) {
  return type === 'gemini' ? 'success' : 'primary'
}

// ---- Quick-add preset ----
type PresetType = 'openai' | 'gemini'

const quickDialogVisible = ref(false)
const quickPreset = ref<PresetType>('openai')
const quickForm = ref({ name: '', base_url: '', api_key: '' })
const quickSubmitting = ref(false)

const presetConfig: Record<PresetType, {
  label: string
  defaultUrl: string
  models: string[]
  mappings: { claude_model: string; target_model: string }[]
}> = {
  openai: {
    label: 'GPT',
    defaultUrl: 'https://yansd666.com',
    models: ['gpt-5.4', 'gpt-5.4-mini'],
    mappings: [
      { claude_model: 'claude-sonnet-4-6', target_model: 'gpt-5.4' },
      { claude_model: 'claude-haiku-4-5-20251000', target_model: 'gpt-5.4-mini' },
    ],
  },
  gemini: {
    label: 'Gemini',
    defaultUrl: 'https://yansd666.com',
    models: ['gemini-3.1-pro-preview', 'gemini-3-flash-preview'],
    mappings: [
      { claude_model: 'claude-sonnet-4-6', target_model: 'gemini-3.1-pro-preview' },
      { claude_model: 'claude-haiku-4-5-20251000', target_model: 'gemini-3-flash-preview' },
    ],
  },
}

function openQuickAdd(type: PresetType) {
  quickPreset.value = type
  const preset = presetConfig[type]
  quickForm.value = { name: preset.label, base_url: preset.defaultUrl, api_key: '' }
  quickDialogVisible.value = true
}

async function submitQuickAdd() {
  const { name, base_url, api_key } = quickForm.value
  if (!name || !base_url || !api_key) {
    ElMessage.warning('请填写所有字段')
    return
  }
  quickSubmitting.value = true
  try {
    const preset = presetConfig[quickPreset.value]
    const provider = await api.addProvider({
      type: quickPreset.value,
      name,
      base_url,
      api_key,
      models: preset.models,
      enabled: true,
    })
    const providerId = provider.id ?? (provider as any).id
    for (const m of preset.mappings) {
      await api.addMapping({ claude_model: m.claude_model, provider_id: providerId, target_model: m.target_model })
    }
    ElMessage.success(`${preset.label} 提供商和模型映射已添加`)
    quickDialogVisible.value = false
    emit('updated')
  } catch {
    ElMessage.error('添加失败')
  } finally {
    quickSubmitting.value = false
  }
}
</script>

<template>
  <div>
    <div class="toolbar">
      <h3 class="section-title">API 提供商</h3>
      <div style="display:flex;gap:8px">
        <el-button type="success" @click="openQuickAdd('openai')">一键添加 GPT</el-button>
        <el-button type="warning" @click="openQuickAdd('gemini')">一键添加 Gemini</el-button>
        <el-button type="primary" :icon="'Plus'" @click="openAdd">添加提供商</el-button>
      </div>
    </div>

    <el-empty v-if="!config.providers.length" description="暂无提供商，点击添加" />

    <el-table v-else :data="config.providers" stripe border style="width:100%">
      <el-table-column label="状态" width="72" align="center">
        <template #default="{ row }">
          <el-switch v-model="row.enabled" @change="toggleEnabled(row)" />
        </template>
      </el-table-column>
      <el-table-column label="类型" width="90">
        <template #default="{ row }">
          <el-tag :type="typeTag(row.type)" size="small">{{ row.type }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="name" label="名称" min-width="120" />
      <el-table-column prop="base_url" label="Base URL" min-width="200" show-overflow-tooltip />
      <el-table-column label="模型列表" min-width="200">
        <template #default="{ row }">
          <el-tag
            v-for="m in row.models"
            :key="m"
            size="small"
            style="margin:2px"
          >{{ m }}</el-tag>
          <span v-if="!row.models.length" class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="120" align="center">
        <template #default="{ row }">
          <el-button size="small" text @click="openEdit(row)">编辑</el-button>
          <el-button size="small" text type="danger" @click="removeProvider(row.id)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <!-- Add/Edit Dialog -->
    <el-dialog
      v-model="dialogVisible"
      :title="isEdit ? '编辑提供商' : '添加提供商'"
      width="560px"
      destroy-on-close
    >
      <el-form :model="form" label-width="110px">
        <el-form-item label="类型" required>
          <el-radio-group v-model="form.type" @change="onTypeChange(form.type)">
            <el-radio-button value="openai">OpenAI 兼容</el-radio-button>
            <el-radio-button value="gemini">Google Gemini</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item label="名称" required>
          <el-input v-model="form.name" placeholder="如：My OpenAI" />
        </el-form-item>
        <el-form-item label="Base URL" required>
          <el-input v-model="form.base_url" placeholder="https://yansd666.com" />
          <div class="hint">
            <span v-if="form.type === 'openai'">
              OpenAI 兼容接口无需填写 /v1，系统会自动补充，如：https://yansd666.com
            </span>
            <span v-else>
              Gemini 默认: https://generativelanguage.googleapis.com，将自动追加模型路径
            </span>
          </div>
        </el-form-item>
        <el-form-item label="API Key" required>
          <el-input
            v-model="form.api_key"
            :type="apiKeyVisible ? 'text' : 'password'"
            placeholder="sk-xxx"
          >
            <template #suffix>
              <el-icon class="cursor-pointer" @click="apiKeyVisible = !apiKeyVisible">
                <component :is="apiKeyVisible ? 'Hide' : 'View'" />
              </el-icon>
            </template>
          </el-input>
        </el-form-item>
        <el-form-item label="可用模型">
          <div style="display:flex;gap:8px;width:100%">
            <el-select
              v-model="form.models"
              filterable
              allow-create
              multiple
              default-first-option
              placeholder="选择或输入模型名后回车添加"
              style="flex:1"
            >
              <el-option
                v-for="m in remoteModels"
                :key="m"
                :label="m"
                :value="m"
              />
            </el-select>
            <el-button
              :loading="fetchingModels"
              @click="fetchRemoteModels"
              :disabled="!form.base_url || !form.api_key"
            >获取模型</el-button>
          </div>
          <div class="hint">可手动输入模型名回车添加，也可点击"获取模型"从远端拉取可用列表</div>
        </el-form-item>
        <el-form-item label="启用">
          <el-switch v-model="form.enabled" />
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" @click="submitForm">保存</el-button>
      </template>
    </el-dialog>

    <!-- Quick-add Dialog -->
    <el-dialog
      v-model="quickDialogVisible"
      :title="`一键添加 ${presetConfig[quickPreset].label} 提供商`"
      width="480px"
      destroy-on-close
    >
      <el-form :model="quickForm" label-width="100px">
        <el-form-item label="名称" required>
          <el-input v-model="quickForm.name" :placeholder="presetConfig[quickPreset].label" />
        </el-form-item>
        <el-form-item label="Base URL" required>
          <el-input v-model="quickForm.base_url" />
        </el-form-item>
        <el-form-item label="API Key" required>
          <el-input v-model="quickForm.api_key" type="password" show-password placeholder="sk-xxx" />
        </el-form-item>
        <el-form-item label="预设模型">
          <el-tag v-for="m in presetConfig[quickPreset].models" :key="m" style="margin:2px">{{ m }}</el-tag>
        </el-form-item>
        <el-form-item label="模型映射">
          <div v-for="m in presetConfig[quickPreset].mappings" :key="m.claude_model" style="font-size:13px;line-height:1.8">
            <el-tag size="small" type="info">{{ m.claude_model }}</el-tag>
            <span style="margin:0 6px">→</span>
            <el-tag size="small">{{ m.target_model }}</el-tag>
          </div>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="quickDialogVisible = false">取消</el-button>
        <el-button type="primary" :loading="quickSubmitting" @click="submitQuickAdd">确认添加</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}
.section-title { margin: 0; font-size: 16px; font-weight: 600; }
.hint { font-size: 12px; color: #999; margin-top: 4px; line-height: 1.4; }
.muted { color: #ccc; }
</style>
