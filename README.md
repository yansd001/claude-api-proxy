# Claude API Proxy

将 Claude Code 的 Anthropic 格式请求转发到 **OpenAI 兼容 API**、**OpenAI Responses API** 或 **Google Gemini API**，并通过 Vue 前端可视化管理配置。

```
Claude Code ──Anthropic格式──▶ 本代理 ──OpenAI/Responses/Gemini格式──▶ 实际 LLM
             ◀──────────────────────── ◀──────────────────────────────────
```

## 功能

- 支持 **OpenAI 兼容 API**（Chat Completions 格式：OpenAI、DeepSeek、硅基流动、火山引擎等）
- 支持 **OpenAI Responses API**（新版 Responses 格式）
- 支持 **Google Gemini API**
- **智能模型路由**：请求模型名包含 `haiku` 时自动走 Haiku 模型映射，否则走默认模型映射
- **单一活跃提供商**：同时只能启用一个提供商，切换时自动禁用其他
- **表格内联编辑**：在提供商列表中直接选择默认模型映射和 Haiku 模型映射，点击时自动获取远端模型列表
- **流式响应**（Streaming）完整支持，含工具调用（Function Calling）
- **Vue 可视化配置界面**，无需手动编辑 JSON
- **Rust 原生编译**，单文件二进制仅约 2.4MB

## 打包为单个可执行文件

> 需要 Rust 工具链（rustup）及 Node.js 18+

```bash
# Windows
publish.bat

# Linux / macOS
bash publish.sh
```

或手动构建：

```bash
# 构建前端
cd frontend && npm install && npm run build && cd ..

# 构建 Rust 后端
cd backend-rust && cargo build --release && cd ..
```

产物位于 `publish/claude-api-proxy.exe`（Windows）或 `publish/claude-api-proxy`（Linux / macOS），以及 `publish/static/` 前端文件。

- 运行后浏览器访问 `http://localhost:8000/ui/` 即可打开配置界面
- `config.json` 会自动在二进制文件同级目录生成，可随程序一并分发

---

## 快速开始（开发模式）

### 1. 启动后端

```bash
cd backend-rust
cargo run
# 默认监听 http://0.0.0.0:8000
```

首次启动会在可执行文件同级目录自动生成 `config.json`，包含一个随机 API Key。

### 2. 启动前端配置界面

```bash
cd frontend
npm install
npm run dev
# 打开 http://localhost:5173
```

### 3. 在配置界面中设置

1. **接入信息** 标签页：查看并复制代理的 `ANTHROPIC_BASE_URL` 和 `ANTHROPIC_API_KEY`。
2. **提供商** 标签页：添加 OpenAI / OpenAI Responses / Gemini 提供商，填写 Base URL 和 API Key。
   - 在提供商列表的「默认模型映射」和「Haiku模型映射」列中直接选择目标模型（点击时自动从远端获取模型列表）。
   - 请求中模型名包含 `haiku` 时走 Haiku 模型映射，否则走默认模型映射；未设置 Haiku 映射时回退到默认模型。
   - 同时只能启用一个提供商。

### 4. 启动 Claude Code

```bash
export ANTHROPIC_BASE_URL=http://localhost:8000
export ANTHROPIC_API_KEY=<配置界面中显示的 Key>
claude
```

或者单次运行：

```bash
ANTHROPIC_BASE_URL=http://localhost:8000 ANTHROPIC_API_KEY=<key> claude
```

## 模型映射说明

每个提供商可配置两个模型映射：

| 映射类型       | 触发条件                         | 示例目标模型            |
| -------------- | -------------------------------- | ----------------------- |
| **默认模型映射** | 请求模型名不含 `haiku`          | `gpt-4o` / `gemini-2.5-pro` |
| **Haiku模型映射** | 请求模型名包含 `haiku`          | `gpt-4o-mini` / `gemini-2.5-flash` |

- 若未设置 Haiku 模型映射，haiku 请求也会走默认模型映射
- 同时只能启用一个提供商，切换启用时其他提供商自动禁用

## 项目结构

```
claude-api-proxy/
├── backend-rust/
│   ├── Cargo.toml                # Rust 项目配置与依赖
│   └── src/
│       ├── main.rs               # Axum 主服务，所有 API 路由
│       ├── config.rs             # 配置文件读写
│       ├── auth.rs               # API Key 鉴权中间件
│       └── converters/
│           ├── mod.rs
│           ├── openai_conv.rs          # Anthropic ↔ OpenAI Chat Completions 格式转换
│           ├── openai_responses_conv.rs # Anthropic ↔ OpenAI Responses 格式转换
│           └── gemini_conv.rs          # Anthropic ↔ Gemini 格式转换
├── frontend/
│   ├── src/
│   │   ├── App.vue
│   │   ├── components/
│   │   │   ├── ServerInfo.vue       # 接入信息与服务器设置
│   │   │   └── ProviderList.vue     # 提供商管理与模型映射
│   │   ├── api.ts            # 与后端的 API 调用封装
│   │   └── types.ts          # TypeScript 类型定义
│   └── package.json
├── publish.bat               # Windows 发布脚本
├── publish.sh                # Linux 发布脚本
└── Dockerfile                # Docker 多阶段构建
```

## Provider Base URL 说明

| 类型              | 默认 Base URL                                        | 实际调用路径                              |
| ----------------- | ---------------------------------------------------- | ----------------------------------------- |
| OpenAI 兼容       | `https://api.openai.com`                             | `{base_url}/v1/chat/completions`          |
| OpenAI Responses  | `https://api.openai.com`                             | `{base_url}/v1/responses`                 |
| Gemini            | `https://generativelanguage.googleapis.com`          | `{base_url}/v1beta/models/{model}:generateContent?key={api_key}` |

OpenAI 兼容的第三方 API 只需修改 Base URL 即可，例如：

- 烟神殿AI: `https://yansd666.com`=

---

## Docker 一键部署

```bash
docker run -d \
  --name claude-api-proxy \
  -p 8000:8000 \
  -v $(pwd)/data:/app/data \
  --restart unless-stopped \
  ghcr.io/yansd001/claude-api-proxy:latest
```

- 启动后访问 `http://<服务器IP>:8000/ui/` 打开配置界面
- `config.json` 自动生成在宿主机 `./data/` 目录中，容器重建后配置不丢失
