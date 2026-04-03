import axios from 'axios'
import type { AnthropicDirect, AppConfig, ModelMapping, Provider, ServerConfig } from './types'

const http = axios.create({ baseURL: '/api' })

export const api = {
  // Full config
  getConfig: () => http.get<AppConfig>('/config').then(r => r.data),
  putConfig: (config: AppConfig) => http.put('/config', config),

  // Providers
  listProviders: () => http.get<Provider[]>('/providers').then(r => r.data),
  addProvider: (p: Omit<Provider, 'id'>) => http.post<Provider>('/providers', p).then(r => r.data),
  updateProvider: (id: string, p: Provider) =>
    http.put<Provider>(`/providers/${id}`, p).then(r => r.data),
  deleteProvider: (id: string) => http.delete(`/providers/${id}`),

  // Model mappings
  listMappings: () => http.get<ModelMapping[]>('/model-mappings').then(r => r.data),
  addMapping: (m: ModelMapping) => http.post<ModelMapping>('/model-mappings', m).then(r => r.data),
  updateMapping: (idx: number, m: ModelMapping) =>
    http.put<ModelMapping>(`/model-mappings/${idx}`, m).then(r => r.data),
  deleteMapping: (idx: number) => http.delete(`/model-mappings/${idx}`),

  // Server settings
  getServer: () => http.get<ServerConfig>('/server').then(r => r.data),
  updateServer: (s: ServerConfig) => http.put('/server', s),

  // Anthropic direct settings
  getAnthropicDirect: () => http.get<AnthropicDirect>('/anthropic-direct').then(r => r.data),
  updateAnthropicDirect: (d: AnthropicDirect) => http.put('/anthropic-direct', d),

  // Fetch remote models from provider
  fetchModels: (base_url: string, api_key: string) =>
    http.post<{ models: string[] }>('/fetch-models', { base_url, api_key }).then(r => r.data),

  // Runtime info
  getRuntimeInfo: () => http.get<{ docker: boolean }>('/runtime-info').then(r => r.data),

  // Claude Code management
  claudeCodeStatus: () => http.get<{ installed: boolean }>('/claude-code/status').then(r => r.data),
  installClaudeCode: () => http.post('/claude-code/install'),
  configureClaudeProxy: () => http.post('/claude-code/configure-proxy'),
  configureClaudeExternal: (base_url: string, api_key: string) =>
    http.post('/claude-code/configure-external', { base_url, api_key }),
}
