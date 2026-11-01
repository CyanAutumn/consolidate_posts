# Upload Hub 插件

默认插件目录为项目根目录的 `plugins/`。主程序启动时只扫描一级子文件夹，每个文件夹读取 `plugin.json`，不会递归把 `src/` 或 `target/` 当作插件。

开发 / 调试构建优先使用工程中的 `plugins/`；分发构建使用 exe 同级的 `plugins/`。可用 `UPLOAD_HUB_PLUGIN_DIR` 指定其他目录，实际位置显示在“平台驱动 → 文件夹插件（全局）”和“工作区管理”。插件程序共用，各工作区单独选择是否使用。新增、更新或修改清单启用状态需重启应用；工作区内勾选已安装插件立即生效。

```text
plugins/
  local-archive/plugin.json    内置本地导出插件
  bilibili/plugin.json         B站，待接入
  xiaohongshu/plugin.json      小红书，待接入
  pixiv/plugin.json            Pixiv 图片投稿，需配置账号
  x/plugin.json                X (Twitter) 浏览器发布，需手动登录
  huggingface/plugin.json      Hugging Face，待接入
  civitai/plugin.json          Civitai，待接入
  template/                  完整 Rust 插件模板，默认禁用
    plugin.json               统一清单
    Cargo.toml
    src/main.rs               统一程序入口 run_plugin(...)
    build-plugin.ps1          编译并生成 plugin.exe
```

## 创建插件

在项目根目录运行：

```powershell
npm run plugin:new -- my-platform "我的平台"
.\plugins\my-platform\build-plugin.ps1
```

创建命令复制模板，同步清单 ID、Rust 入口 ID 和 Cargo 包名；已有同名文件夹会拒绝覆盖。编译后重启主程序，新插件出现在全局列表，无需改主程序注册代码。在“工作区管理”中勾选后才能用于该区的发布和补传；不会自动加入已有工作区。

模板是一个**实际可运行的本地导出示例**，不会伪造远端上传。可以先测试普通上传和历史补传，再替换 `src/main.rs` 中的 `validate`、`upload` 接入真实平台。模板目录本身 `enabled: false`，不会作为可选平台。

使用 Rust MSVC 工具链构建。`upload-plugin-sdk` 使用本仓库的路径依赖，因此源代码构建应在项目的 `plugins/<目录>/` 中进行；分发已编译插件只需保留清单、`plugin.exe` 和插件自己使用的资源，不需要用户安装 Rust。

## 统一入口

每个进程插件都是普通可执行文件，通过以下唯一入口启动：

```text
plugin.exe --upload-hub-plugin
```

一个进程处理一个请求：主程序将一行 UTF-8 JSON 写入 stdin 并关闭输入，插件通过 stdout 返回 JSON Lines，然后退出。日志写到 stderr。Rust 模板调用 `upload_plugin_sdk::run_plugin(YourPlugin)`，SDK 负责解析协议、分派 `validate` / `upload`、输出进度和错误。

插件可以用其他语言实现，只要提供同样协议的可执行文件。Windows 入口必须是插件目录内的 `.exe`，不通过 shell 拼接命令；入口路径不可跨出该插件文件夹。主程序以无窗口方式启动插件。

`local-archive` 是保留的内置插件，清单入口为 `{"type":"builtin","name":"local-archive"}`，通过同一个 `UploadDriver` 接口调用。强制留档本身属于核心流程，禁用本地导出插件也不会关闭自动留档。

## 清单字段

| 字段 | 说明 |
| --- | --- |
| `schemaVersion` | 当前为 `1` |
| `id` | 唯一且稳定，1–64 位小写字母、数字、短横线；修改会被视为新平台 |
| `version` / `name` / `category` / `description` | 插件版本和界面信息 |
| `enabled` | `false` 表示不加载，也不创建新账号目标 |
| `status` | `ready` 可执行；`planned` 为尚未接入的平台，可暂时没有入口文件 |
| `entry` | 外部插件为 `{"type":"process","path":"plugin.exe"}` |
| `kinds` | `image`、`video`、`text`、`model`、`file` 的子集 |
| `acceptsTitle` / `acceptsBody` | 是否支持标题和正文 |
| `maxFiles` / `maxFileBytes` | 附件数量和单文件字节限制，`null` 不设此项限制 |
| `timeoutSeconds` | 上传超时，1–86400 秒，默认 7200；校验请求固定最多 10 秒 |

清单坏损、缺少入口、协议版本不支持和 ID 冲突会单独显示诊断。重复 ID 的所有插件均不执行，不会根据扫描顺序随机选一个。已有历史、账号和任务不会因此删除；无法加载的插件不能接收新上传。

## 协议 v1

请求示例（实际是一行 JSON）：

```json
{
  "protocolVersion": 1,
  "pluginId": "my-platform",
  "operation": "upload",
  "publication": { "title": "标题", "body": "正文", "assets": [] },
  "target": { "id": "账号目标ID", "driverId": "my-platform", "name": "新账号", "createdAt": 0 },
  "jobId": "由主程序生成的任务UUID",
  "operationId": "原始操作UUID（重试时保持不变）",
  "outputDir": "C:\\UploadHubData\\workspaces\\工作区UUID\\deliveries\\账号目标ID",
  "workspace": {
    "id": "工作区UUID",
    "name": "R18 创作",
    "contentRating": "r18",
    "dataDir": "C:\\UploadHubData\\workspaces\\工作区UUID\\plugin-data\\my-platform"
  }
}
```

`validate` 请求只检查内容，不得发布或修改远端；其 `target`、`jobId`、`operationId`、`outputDir`、`workspace` 为 `null`。账号授权等检查应在实际上传阶段执行。`upload` 请求携带目标账号和任务上下文；`publication.assets` 始终是已归档文件的绝对路径和校验值，不会把整个大文件编码成 JSON。

`workspace` 是向后兼容的可选字段，旧请求可省略。桌面版上传时会提供当前区 ID、名称、`contentRating`（`unspecified` / `all_ages` / `r18`）以及已创建的专用 `dataDir`。Rust 插件从 `context.workspace` 读取。分类为用户设置的工作区信息，平台字段如何使用由插件决定。

附件字段为 `id`、`path`、`name`、`kind`、`size`、`modifiedMs`、`sha256`。插件应流式读取附件，避免一次性读入大模型。账号配置应按 `workspace.dataDir/<target.id>` 保存，禁止把不同工作区的账号状态混写在插件安装目录；默认 `target.id` 可能跨区重复。当前主程序账号是本机标识，还没有统一凭证管理接口。

stdout 响应示例：

```json
{"protocolVersion":1,"type":"validation","issues":[]}
```

或在上传时输出零到多行进度，最后一行结果：

```json
{"protocolVersion":1,"type":"progress","completedBytes":1024}
{"protocolVersion":1,"type":"result","receipt":{"location":"https://example.com/作品地址","files":[]}}
```

失败返回：

```json
{"protocolVersion":1,"type":"error","error":{"code":"authorization_required","message":"请先完成账号授权"}}
```

每条消息最多 2 MiB。进度不得倒退或超过附件总字节数；结果必须与请求类型匹配，只能有一个最终响应，之后应关闭输出并以状态码 0 退出。应用错误可以用 `error` 响应并正常退出；崩溃、非零退出、非法 JSON、缺失结果或超时会由主程序记为失败。

取消 / 超时会终止当前插件进程，应用退出时也会请求取消。外部插件应将中间文件放在任务专属目录，不得遗留继承标准输入输出管道的后台子进程。进程终止可能留下中间文件；远端已提交的作品不会因此撤回，驱动应使用幂等键（使用跨重试稳定的 `operationId`，旧请求缺省时 SDK 回退到 `jobId`）并保存必要回执。

插件是以当前用户权限运行的本机程序，不是权限沙箱；协议的路径检查与超时不限制插件内部代码的文件或网络访问。

## 与历史补传的关系

插件被发现后会在各区建立默认账号目标；只有在该区启用后才可使用。同一插件可以继续添加多个目标，每区按 `archiveId + targetId` 分别记录上传状态。历史补传、定时间隔、去重和重试由主程序统一处理；插件通过同一个 `upload` 入口接收当前区的归档，无需实现第二套历史导入逻辑。插件的持久化状态也应使用上述工作区数据目录。

分发主程序使用 `npm run desktop:build` 或 `npm run desktop:build:debug`，会把插件目录复制到 exe 旁边，跳过模板和构建缓存。该步骤复制 / 更新已有文件，不自动删除目标目录中额外安装的插件。直接执行原始 `tauri build` 时，需要自行携带 `plugins/`。

协议类型：`crates/upload-core/src/plugins/protocol.rs`；Rust 入口 SDK：`crates/upload-plugin-sdk/src/lib.rs`。

## 现有网络插件

[Pixiv 图片插件](pixiv/README.md) 复用统一入口，支持 CloakBrowser 窗口手动登录、自动读取并加密保存凭据，保留手动 Cookie 配置方式；上传使用流式 multipart、确认回执及提交日志。真实账号上传尚未验证。

[X (Twitter) 浏览器插件](x/README.md) 使用 CloakBrowser + Playwright，通过独立浏览器配置保存登录态，支持文字、图片、GIF 和 MP4，复用统一插件协议及定时补传。其原生 `.exe` 入口负责账号互斥和取消时关闭完整进程树，Node 运行依赖随插件分发。真实账号发布尚未验证。
