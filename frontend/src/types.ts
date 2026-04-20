export type ProviderType = 'openai' | 'openai_responses' | 'gemini'

export interface Provider {
  id: string
  type: ProviderType
  name: string
  base_url: string
  api_key: string
  models: string[]   // list of available model names on this provider
  enabled: boolean
  default_model: string   // default model mapping
  haiku_model: string     // haiku model mapping
}

export interface ModelMapping {
  claude_model: string   // what Claude Code sends, e.g. "claude-sonnet-4-6"
  provider_id: string
  target_model: string   // what gets forwarded to the provider
}

export interface ServerConfig {
  host: string
  port: number
  api_key: string
}

export interface AnthropicDirect {
  enabled: boolean
  base_url: string
  api_key: string
}

export interface AppConfig {
  server: ServerConfig
  anthropic_direct: AnthropicDirect
  providers: Provider[]
  model_mappings: ModelMapping[]
  default_provider_id: string
  default_model: string
}
