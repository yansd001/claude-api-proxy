# Claude API Proxy

将 Claude Code 的 Anthropic 格式请求转发到 **OpenAI 兼容 API** 或 **Google Gemini API**，并通过 Vue 前端可视化管理配置。

```
Claude Code ──Anthropic格式──▶ 本代理 ──OpenAI/Gemini格式──▶ 实际 LLM
             ◀──────────────────────── ◀──────────────────────────
```

## 功能

- 支持 **OpenAI 兼容 API**（OpenAI、DeepSeek、硅基流动、火山引擎等）
- 支持 **Google Gemini API**
- **模型映射**：将 Claude Code 发送的模型名（如 `claude-sonnet-4-6`）映射到任意目标模型
- **流式响应**（Streaming）完整支持，含工具调用（Function Calling）
- **Vue 可视化配置界面**，无需手动编辑 JSON

## 打包为单个可执行文件（.exe）

> 需要 Python 3.11+ 及 Node.js 18+

```bash
# 安装打包依赖
pip install -r backend/requirements.txt
pip install -r backend/build-requirements.txt

# 一键构建（自动完成 npm build + PyInstaller）
python build.py
```

产物位于 `dist/claude-api-proxy.exe`（Windows）或 `dist/claude-api-proxy`（macOS / Linux）。

- 双击运行后浏览器访问 `http://localhost:8000/ui/` 即可打开配置界面
- `config.json` 会自动在 exe 同级目录生成，可随 exe 一并分发

---

## 快速开始（开发模式）

### 1. 启动后端

```bash
cd backend
pip install -r requirements.txt
python main.py
# 默认监听 http://0.0.0.0:8000
```

首次启动会在 `backend/config.json` 中自动生成一个随机 API Key。

### 2. 启动前端配置界面

```bash
cd frontend
npm install
npm run dev
# 打开 http://localhost:5173
```

### 3. 在配置界面中设置

1. **接入信息** 标签页：查看并复制代理的 `ANTHROPIC_BASE_URL` 和 `ANTHROPIC_API_KEY`。
2. **提供商** 标签页：添加 OpenAI 或 Gemini 提供商，填写 Base URL、API Key 和可用模型列表。
3. **模型映射** 标签页：设置 Claude Code 模型名到实际提供商模型的映射关系，并配置默认路由。

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

## 模型映射示例

| Claude Code 发送的模型     | 转发到提供商    | 目标模型名              |
| -------------------------- | -------------- | ----------------------- |
| `claude-sonnet-4-6`        | OpenAI         | `gpt-4o`               |
| `claude-3-5-haiku-20241022`| Gemini         | `gemini-2.5-flash`     |
| `claude-opus-4-5`          | DeepSeek       | `deepseek-chat`        |

未命中映射时，使用"默认路由"中设置的提供商和模型。

## 项目结构

```
claude-api-proxy/
├── backend/
│   ├── main.py               # FastAPI 主服务，/v1/messages 端点
│   ├── config_manager.py     # 配置文件读写
│   ├── auth.py               # API Key 鉴权
│   ├── converters/
│   │   ├── openai_conv.py    # Anthropic ↔ OpenAI 格式转换
│   │   └── gemini_conv.py    # Anthropic ↔ Gemini 格式转换
│   ├── config.json           # 运行时配置（自动生成）
│   └── requirements.txt
└── frontend/
    ├── src/
    │   ├── App.vue
    │   ├── components/
    │   │   ├── ServerInfo.vue       # 接入信息与服务器设置
    │   │   ├── ProviderList.vue     # 提供商管理
    │   │   └── ModelMappings.vue    # 模型映射管理
    │   ├── api.ts            # 与后端的 API 调用封装
    │   └── types.ts          # TypeScript 类型定义
    └── package.json
```

## Provider Base URL 说明

| 类型   | 默认 Base URL                                        | 实际调用路径                              |
| ------ | ---------------------------------------------------- | ----------------------------------------- |
| OpenAI | `https://api.openai.com/v1`                          | `{base_url}/chat/completions`             |
| Gemini | `https://generativelanguage.googleapis.com`          | `{base_url}/v1beta/models/{model}:generateContent?key={api_key}` |

OpenAI 兼容的第三方 API 只需修改 Base URL 即可，例如：

- DeepSeek: `https://api.deepseek.com/v1`
- 硅基流动: `https://api.siliconflow.cn/v1`
- 火山引擎: `https://ark.cn-beijing.volces.com/api/v3`

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
