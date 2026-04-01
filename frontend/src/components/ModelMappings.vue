<script setup lang="ts">
import { computed, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { api } from '../api'
import type { AppConfig, ModelMapping, Provider } from '../types'

const props = defineProps<{ config: AppConfig }>()
const emit = defineEmits<{ updated: [] }>()

// ---- enriched mappings with provider name ----
const mappingsWithName = computed(() => {
  return props.config.model_mappings.map((m, idx) => {
    const p = props.config.providers.find(p => p.id === m.provider_id)
    return { ...m, _idx: idx, _providerName: p?.name ?? '(已删除)', _providerType: p?.type ?? '' }
  })
})

const providerOptions = computed(() => props.config.providers.filter(p => p.enabled))

// ---- Dialog ----
const dialogVisible = ref(false)
const isEdit = ref(false)
const editingIdx = ref(-1)

const emptyForm = (): ModelMapping => ({
  claude_model: '',
  provider_id: '',
  target_model: '',
})

const form = ref<ModelMapping>(emptyForm())

const selectedProviderModels = computed<string[]>(() => {
  const p = props.config.providers.find(p => p.id === form.value.provider_id)
  return p?.models ?? []
})

function openAdd() {
  isEdit.value = false
  form.value = emptyForm()
  dialogVisible.value = true
}

function openEdit(row: ModelMapping & { _idx: number }) {
  isEdit.value = true
  editingIdx.value = row._idx
  form.value = { ...row }
  dialogVisible.value = true
}

async function submit() {
  if (!form.value.claude_model || !form.value.provider_id || !form.value.target_model) {
    ElMessage.warning('请填写所有字段')
    return
  }
  try {
    if (isEdit.value) {
      await api.updateMapping(editingIdx.value, form.value)
      ElMessage.success('映射已更新')
    } else {
      await api.addMapping(form.value)
      ElMessage.success('映射已添加')
    }
    dialogVisible.value = false
    emit('updated')
  } catch {
    ElMessage.error('操作失败')
  }
}

async function removeMapping(idx: number) {
  try {
    await ElMessageBox.confirm('确定删除该模型映射？', '确认删除', { type: 'warning' })
    await api.deleteMapping(idx)
    ElMessage.success('已删除')
    emit('updated')
  } catch {
    // cancelled
  }
}

// Built-in Claude model suggestions
const claudeModelSuggestions = [
  'claude-opus-4-5', 'claude-sonnet-4-5', 'claude-haiku-4-5',
  'claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-6',
  'claude-3-5-sonnet-20241022', 'claude-3-5-haiku-20241022',
  'claude-3-opus-20240229',
]
</script>

<template>
  <div>
    <!-- Mapping table -->
    <div class="toolbar">
      <h3 class="section-title">模型映射规则</h3>
      <el-button type="primary" :icon="'Plus'" @click="openAdd">添加映射</el-button>
    </div>

    <el-alert type="info" :closable="false" style="margin-bottom:16px">
      当 Claude Code 使用指定的模型名时，自动转发到对应提供商的模型。<br/>
      <strong>若无任何映射规则，所有请求将自动转发到第一个可用提供商的第一个模型。</strong>
    </el-alert>

    <el-empty v-if="!mappingsWithName.length" description="暂无映射规则，点击添加" />

    <el-table v-else :data="mappingsWithName" stripe border>
      <el-table-column label="Claude Code 中的模型名" prop="claude_model" min-width="200">
        <template #default="{ row }">
          <code class="model-name">{{ row.claude_model }}</code>
        </template>
      </el-table-column>
      <el-table-column label="→" width="40" align="center">
        <template #default><el-icon><Right /></el-icon></template>
      </el-table-column>
      <el-table-column label="提供商" min-width="130">
        <template #default="{ row }">
          <el-tag
            :type="row._providerType === 'gemini' ? 'success' : 'primary'"
            size="small"
          >{{ row._providerType }}</el-tag>
          {{ row._providerName }}
        </template>
      </el-table-column>
      <el-table-column label="目标模型" prop="target_model" min-width="180">
        <template #default="{ row }">
          <code class="model-name">{{ row.target_model }}</code>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="120" align="center">
        <template #default="{ row }">
          <el-button size="small" text @click="openEdit(row)">编辑</el-button>
          <el-button size="small" text type="danger" @click="removeMapping(row._idx)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <!-- Dialog -->
    <el-dialog
      v-model="dialogVisible"
      :title="isEdit ? '编辑模型映射' : '添加模型映射'"
      width="500px"
      destroy-on-close
    >
      <el-form :model="form" label-width="160px">
        <el-form-item label="Claude Code 模型名" required>
          <el-select
            v-model="form.claude_model"
            filterable
            allow-create
            placeholder="如 claude-sonnet-4-6"
            style="width:100%"
          >
            <el-option
              v-for="m in claudeModelSuggestions"
              :key="m"
              :label="m"
              :value="m"
            />
          </el-select>
          <div class="hint">Claude Code 发送请求时使用的模型名称</div>
        </el-form-item>
        <el-form-item label="转发到提供商" required>
          <el-select
            v-model="form.provider_id"
            placeholder="选择提供商"
            style="width:100%"
          >
            <el-option
              v-for="p in providerOptions"
              :key="p.id"
              :label="`[${p.type}] ${p.name}`"
              :value="p.id"
            />
          </el-select>
        </el-form-item>
        <el-form-item label="目标模型名" required>
          <el-select
            v-model="form.target_model"
            filterable
            allow-create
            placeholder="实际调用的模型名"
            style="width:100%"
          >
            <el-option
              v-for="m in selectedProviderModels"
              :key="m"
              :label="m"
              :value="m"
            />
          </el-select>
          <div class="hint">实际发送给提供商 API 的模型名称</div>
        </el-form-item>
      </el-form>

      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" @click="submit">保存</el-button>
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
.section-block { margin-bottom: 8px; }
.model-name {
  font-family: monospace;
  font-size: 13px;
  background: #f5f5f5;
  padding: 2px 6px;
  border-radius: 4px;
}
.hint { font-size: 12px; color: #999; margin-top: 4px; }
</style>
