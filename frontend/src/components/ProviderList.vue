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
const remoteModels = ref<string[]>([])

const emptyForm = (): Omit<Provider, 'id'> => ({
  type: 'openai',
  name: '',
  base_url: '',
  api_key: '',
  models: [],
  enabled: true,
  default_model: '',
  haiku_model: '',
})

const form = ref(emptyForm())

const defaultBaseUrls: Record<ProviderType, string> = {
  openai: 'https://api.openai.com',
  gemini: 'https://generativelanguage.googleapis.com',
  openai_responses: 'https://api.openai.com',
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
    // Also update the models list on the form
    form.value.models = result.models
    ElMessage.success(`获取到 ${result.models.length} 个模型`)
  } catch (e: any) {
    ElMessage.error(e?.response?.data?.detail || '获取模型列表失败，请检查 Base URL 和 API Key')
  } finally {
    fetchingModels.value = false
  }
}

// Auto-fetch when opening model select dropdown
async function onModelSelectFocus() {
  if (remoteModels.value.length === 0 && form.value.base_url && form.value.api_key) {
    await fetchRemoteModels()
  }
}

async function submitForm() {
  if (!form.value.name || !form.value.base_url || !form.value.api_key) {
    ElMessage.warning('请填写所有必填字段')
    return
  }
  if (!form.value.default_model) {
    ElMessage.warning('请选择默认模型映射')
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
    await ElMessageBox.confirm('确定删除该提供商？', '确认删除', {
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
    // Enforce single enabled: if enabling, disable others in UI first
    if (p.enabled) {
      for (const other of props.config.providers) {
        if (other.id !== p.id) {
          other.enabled = false
        }
      }
    }
    await api.updateProvider(p.id, p)
    emit('updated')
  } catch {
    ElMessage.error('操作失败')
  }
}

function typeTag(type: ProviderType) {
  if (type === 'gemini') return 'success'
  if (type === 'openai_responses') return 'warning'
  return 'primary'
}

function typeLabel(type: ProviderType) {
  if (type === 'openai') return 'OpenAI'
  if (type === 'gemini') return 'Gemini'
  if (type === 'openai_responses') return 'OpenAI Responses'
  return type
}

// ---- Inline model selection in table ----
const inlineModels = ref<Record<string, string[]>>({})
const inlineFetching = ref<Record<string, boolean>>({})

async function fetchInlineModels(p: Provider) {
  if (inlineModels.value[p.id]?.length) return
  if (!p.base_url || !p.api_key) return
  inlineFetching.value[p.id] = true
  try {
    const result = await api.fetchModels(p.base_url, p.api_key)
    inlineModels.value[p.id] = result.models
  } catch {
    // silently fail, user can still type
  } finally {
    inlineFetching.value[p.id] = false
  }
}

async function onInlineModelChange(p: Provider) {
  try {
    await api.updateProvider(p.id, p)
    emit('updated')
  } catch {
    ElMessage.error('更新失败')
  }
}

// ---- Quick-add preset ----
const quickDialogVisible = ref(false)
const quickType = ref<ProviderType>('openai')
const quickForm = ref({ name: '', base_url: '', api_key: '' })
const quickSubmitting = ref(false)

const quickPresetModels: Record<ProviderType, { default_model: string; haiku_model: string }> = {
  openai: { default_model: 'gpt-5.4', haiku_model: 'gpt-5.4-mini' },
  gemini: { default_model: 'gemini-3.1-pro-preview', haiku_model: 'gemini-3-flash-preview' },
  openai_responses: { default_model: 'gpt-5.5', haiku_model: 'gpt-5.5-mini' },
}

const quickTypeLabels: Record<ProviderType, string> = {
  openai: 'OpenAI 兼容',
  gemini: 'Google Gemini',
  openai_responses: 'OpenAI Responses',
}

function openQuickAdd() {
  quickType.value = 'openai'
  quickForm.value = { name: 'OpenAI 兼容', base_url: 'https://yansd666.com', api_key: '' }
  quickDialogVisible.value = true
}

function onQuickTypeChange(type: ProviderType) {
  quickForm.value.name = quickTypeLabels[type]
  quickForm.value.base_url = 'https://yansd666.com'
}

async function submitQuickAdd() {
  const { name, base_url, api_key } = quickForm.value
  if (!name || !base_url || !api_key) {
    ElMessage.warning('请填写所有字段')
    return
  }
  quickSubmitting.value = true
  try {
    const models = quickPresetModels[quickType.value]
    await api.addProvider({
      type: quickType.value,
      name,
      base_url,
      api_key,
      models: [models.default_model, models.haiku_model],
      enabled: true,
      default_model: models.default_model,
      haiku_model: models.haiku_model,
    })
    ElMessage.success(`${quickTypeLabels[quickType.value]} 提供商已添加`)
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
      <el-button type="success" @click="openQuickAdd">一键添加</el-button>
    </div>

    <el-alert type="info" :closable="false" style="margin-bottom:16px">
      只能启用一个提供商。请求中模型名包含 <strong>haiku</strong> 时走 Haiku 模型映射，否则走默认模型映射。
    </el-alert>

    <el-empty v-if="!config.providers.length" description="暂无提供商，点击添加" />

    <el-table v-else :data="config.providers" stripe border style="width:100%">
      <el-table-column label="启用" width="72" align="center">
        <template #default="{ row }">
          <el-switch v-model="row.enabled" @change="toggleEnabled(row)" />
        </template>
      </el-table-column>
      <el-table-column label="类型" width="100">
        <template #default="{ row }">
          <el-tag :type="typeTag(row.type)" size="small">{{ typeLabel(row.type) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="name" label="名称" min-width="100" />
      <el-table-column prop="base_url" label="Base URL" min-width="180" show-overflow-tooltip />
      <el-table-column label="默认模型映射" min-width="180">
        <template #default="{ row }">
          <el-select
            v-model="row.default_model"
            filterable
            allow-create
            default-first-option
            placeholder="选择模型"
            size="small"
            style="width:100%"
            :loading="inlineFetching[row.id]"
            @focus="fetchInlineModels(row)"
            @change="onInlineModelChange(row)"
          >
            <el-option v-for="m in (inlineModels[row.id] || row.models || [])" :key="m" :label="m" :value="m" />
          </el-select>
        </template>
      </el-table-column>
      <el-table-column label="Haiku模型映射" min-width="180">
        <template #default="{ row }">
          <el-select
            v-model="row.haiku_model"
            filterable
            allow-create
            clearable
            default-first-option
            placeholder="选择模型"
            size="small"
            style="width:100%"
            :loading="inlineFetching[row.id]"
            @focus="fetchInlineModels(row)"
            @change="onInlineModelChange(row)"
          >
            <el-option v-for="m in (inlineModels[row.id] || row.models || [])" :key="m" :label="m" :value="m" />
          </el-select>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="140" align="center" :resizable="false">
        <template #default="{ row }">
          <div style="white-space:nowrap">
            <el-button size="small" text @click="openEdit(row)">编辑</el-button>
            <el-button size="small" text type="danger" @click="removeProvider(row.id)">删除</el-button>
          </div>
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
      <el-form :model="form" label-width="120px">
        <el-form-item label="类型" required>
          <el-radio-group v-model="form.type" @change="onTypeChange(form.type)">
            <el-radio-button value="openai">OpenAI 兼容</el-radio-button>
            <el-radio-button value="openai_responses">OpenAI Responses</el-radio-button>
            <el-radio-button value="gemini">Google Gemini</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item label="名称" required>
          <el-input v-model="form.name" placeholder="如：My OpenAI" />
        </el-form-item>
        <el-form-item label="Base URL" required>
          <el-input v-model="form.base_url" placeholder="https://api.openai.com" />
          <div class="hint">
            <span v-if="form.type === 'openai'">
              OpenAI 兼容接口无需填写 /v1，系统会自动补充
            </span>
            <span v-else-if="form.type === 'openai_responses'">
              OpenAI Responses API，将使用 /v1/responses 端点，无需填写 /v1
            </span>
            <span v-else>
              Gemini 默认: https://generativelanguage.googleapis.com
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
        <el-form-item label="默认模型映射" required>
          <el-select
            v-model="form.default_model"
            filterable
            allow-create
            default-first-option
            placeholder="点击选择（自动获取模型列表）"
            style="width:100%"
            :loading="fetchingModels"
            @focus="onModelSelectFocus"
          >
            <el-option
              v-for="m in remoteModels"
              :key="m"
              :label="m"
              :value="m"
            />
          </el-select>
          <div class="hint">非 haiku 请求将使用此模型</div>
        </el-form-item>
        <el-form-item label="Haiku 模型映射">
          <el-select
            v-model="form.haiku_model"
            filterable
            allow-create
            clearable
            default-first-option
            placeholder="点击选择（自动获取模型列表）"
            style="width:100%"
            :loading="fetchingModels"
            @focus="onModelSelectFocus"
          >
            <el-option
              v-for="m in remoteModels"
              :key="m"
              :label="m"
              :value="m"
            />
          </el-select>
          <div class="hint">模型名含 haiku 时使用此模型，为空则回退到默认模型</div>
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
      title="一键添加提供商"
      width="480px"
      destroy-on-close
    >
      <el-form :model="quickForm" label-width="100px">
        <el-form-item label="提供商类型" required>
          <el-radio-group v-model="quickType" @change="onQuickTypeChange">
            <el-radio-button value="openai">OpenAI 兼容</el-radio-button>
            <el-radio-button value="openai_responses">OpenAI Responses</el-radio-button>
            <el-radio-button value="gemini">Google Gemini</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item label="名称" required>
          <el-input v-model="quickForm.name" :placeholder="quickTypeLabels[quickType]" />
        </el-form-item>
        <el-form-item label="Base URL" required>
          <el-input v-model="quickForm.base_url" />
        </el-form-item>
        <el-form-item label="API Key" required>
          <el-input v-model="quickForm.api_key" type="password" show-password placeholder="sk-xxx" />
        </el-form-item>
        <el-form-item label="模型映射">
          <div style="font-size:13px;line-height:1.8">
            <div>
              <el-tag size="small" type="info">默认</el-tag>
              <span style="margin:0 6px">→</span>
              <el-tag size="small">{{ quickPresetModels[quickType].default_model }}</el-tag>
            </div>
            <div>
              <el-tag size="small" type="info">Haiku</el-tag>
              <span style="margin:0 6px">→</span>
              <el-tag size="small">{{ quickPresetModels[quickType].haiku_model }}</el-tag>
            </div>
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
.model-name { font-size: 12px; background: #f5f5f5; padding: 2px 6px; border-radius: 3px; }
</style>
