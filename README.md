# Upload Hub · 聚合上传工作台

沿用 `C:\Users\Admin\Desktop\CyanTools2` 的 **React 19 + TypeScript + Vite + Tauri 2 + Rust + SQLite** 技术路线。项目在当前目录独立搭建，未修改 CyanTools2；上传业务直接在 Rust 中执行，不需要 Python 或 Node 后端服务。

把图片、视频、文本、模型和其他文件组合成一次发布，填写标题、正文，再选择兼容的平台。平台插件默认位于 `plugins/`，每个子文件夹一个插件，通过统一入口接入队列、进度、取消和历史补传。

## 当前已实现

- **多个独立工作区**：分别选择 R18 / 全年龄等分类和插件，保存各自的草稿、平台勾选、归档、上传记录及定时计划。切换前自动保存草稿，后台任务继续运行；旧数据保留在默认工作区。
- 原生文件选择、多文件拖入、扩展名分类、去重、逐文件导入错误反馈；导入仅保存路径，不搬动源文件。
- 标题 / 正文 / 附件草稿，手动保存到 SQLite，提交任务时同时保存。
- 按驱动能力判断整份内容是否兼容；Rust 再次验证来源、文件大小、修改时间和能力限制。
- 每次发布先生成本地归档；归档成功后，才为选中的平台账号执行上传。每个工作区使用两个后台工作线程从自己的 SQLite 领取到期任务。
- 取消排队或执行中的任务；失败、取消、中断可单独手动重试，保留旧尝试。
- **自动本地留档**：1 MiB 分块读写，计算 SHA-256，保存附件副本、标题、正文和 `manifest.json`。归档失败会阻止后续上传；不选任何目标可“仅保存归档”。
- **历史补传**：全选或勾选历史，选择平台，立即执行或指定开始时间；可设置每条间隔，例如 60 分钟或 1440 分钟。支持暂停、继续、取消。
- **按平台记录结果**：默认跳过该账号已成功或正在排队的历史；可显式重复已成功内容，仍禁止重复排队。补传复用本地副本，原文件删除后仍可使用。
- **单账号平台登录**：Pixiv 和 X 每个平台只显示一个账号，点击登录即可；自动沿用旧版已保存的登录配置。
- 发布工作台、上传任务、历史归档、平台登录、工作区管理五个页面。
- **文件夹插件**：启动扫描 `plugins/*/plugin.json`；支持外部可执行插件、禁用、版本和入口检查、重复 ID 诊断。提供统一 JSON Lines 协议及 Rust SDK / 模板。

**Pixiv 图片插件已实现网页接口投稿；X (Twitter) 插件通过独立浏览器登录态发布文字、图片、GIF 和 MP4。两者需配置账号，尚未实号发布验证。** 见 [Pixiv 使用说明](plugins/pixiv/README.md)和 [X 使用说明](plugins/x/README.md)。B站、小红书、Hugging Face、Civitai 为计划驱动，尚未接入账号登录或真实上传；页面中已禁用这些平台的提交。本地归档不会向互联网发送任何内容。

## 启动

开发需要 Node.js（本机验证 v24）、Rust MSVC 工具链、Microsoft C++ Build Tools、WebView2。详细系统依赖见 [Tauri 官方先决条件](https://v2.tauri.app/start/prerequisites/)。

在本项目目录运行：

```powershell
npm ci
npm run plugin:build
npm run desktop:dev
```

也可以双击 `run_update.bat`，它复用现有的 `run_upload_hub.bat` 启动流程：按需安装前端依赖，构建最新的独立调试版再启动，不占用开发端口。使用 `run_update.bat --dev` 可进入带热更新的桌面开发模式；原有的 `run_upload_hub.bat` 仍可使用。

只查看界面：

```powershell
npm run dev
```

打开 <http://127.0.0.1:1430>。该端口与 CyanTools2 的 1420 错开。浏览器模式明确标识为预览，文件导入、保存和上传不可用，不伪造后端结果。不要同时运行本项目的 `dev` 和 `desktop:dev`，后者会自行启动 Vite。

生成独立桌面程序（目前不生成安装包）：

```powershell
npm run desktop:build
# 产物：target/release/upload-hub.exe
```

调试版独立程序：

```powershell
npm run desktop:build:debug
# 产物：target/debug/upload-hub.exe
```

构建命令会先构建 Pixiv 与 X 插件，再复制到 exe 旁的 `plugins/`，分发时需一起携带。Pixiv 浏览器登录与 X 插件的 Node 运行依赖会一起复制，浏览器功能仍需安装 Node.js；浏览器组件在首次账号配置时下载到用户缓存。开发 / 调试构建优先读取工程 `plugins/`；也可通过 `UPLOAD_HUB_PLUGIN_DIR` 指定目录，实际位置见界面。

## 新建插件

```powershell
npm run plugin:new -- my-platform "我的平台"
.\plugins\my-platform\build-plugin.ps1
```

模板位于 `plugins/template/`，包含 `plugin.json`、`src/main.rs`、`Cargo.toml` 和构建脚本。复制生成后只需实现 `validate` / `upload`，统一入口为 `run_plugin(YourPlugin)`。模板可直接运行并做本地导出；真实平台接口需要在插件内接入。新增、更新或启用后重启主程序，不需要修改平台注册表。完整字段与协议见 [插件开发说明](plugins/README.md)。

## 体验实际流程

先在“工作区管理”中新建空间，填写名称 / 分类并选择插件。比如 R18 区只选指定平台，全年龄区可全选插件；左侧或窄屏顶部的选择框用于切换。新空间初始不勾选发布账号，需在工作台选择。详细隔离与保存规则见 [多个工作区](docs/workspaces.md)。

1. 在桌面版拖入文件或点击导入，填写标题 / 正文；也可以仅填写文字。
2. 选中“本地导出”后“一键上传”，或取消目标选择后“仅保存归档”。
3. 在“历史归档”查看保存内容；“上传任务”分别显示留档和上传结果。
4. 在“平台登录”登录需要的平台，点击该平台的“补传历史”。
5. 全选历史，设置开始时间和间隔，点击“检查补传计划”，再“确认创建计划”。详细规则见 [归档与定时补传](docs/archive-replay.md)。

数据写在 Tauri 的 `app_data_dir()` 下；Windows 默认通常为 `%APPDATA%\com.uploadhub.desktop`，准确位置见“工作区管理”。测试或隔离运行可设置 `UPLOAD_HUB_DATA_DIR`。默认工作区保留原目录，新增工作区使用 `workspaces/<工作区 UUID>/`。每区的数据库为 `upload-hub.sqlite3`，归档位于 `archives/<归档 UUID>/`，本地导出位于 `deliveries/<账号 ID>/<任务 UUID>/`。根目录 `workspaces.sqlite3` 保存工作区索引及上次选择。同一数据目录有进程锁，避免多个实例重复派发。

**定时执行需要保持应用开启，不能自动唤醒电脑。** 关闭后待执行计划保留，重启或恢复后按实际派发时间继续保持间隔，不会集中补发错过的全部任务。运行中断的任务需核对后手动重试。已完成旧版本本地归档会自动纳入历史。

导入仍只登记源文件；正式提交后才复制为归档。源文件发生变化时会报错，需要重新导入。补传前会重新核对归档的 SHA-256。当前通过扩展名识别类型，后续平台驱动还需验证实际文件格式。

## 结构与验证

```text
src/                         React 界面、状态与 Tauri IPC 客户端
plugins/<插件目录>/plugin.json 平台能力、版本、状态和统一入口清单
plugins/template/            可独立编译的 Rust 插件模板
crates/upload-plugin-sdk/     统一入口 SDK 与真实插件进程测试
crates/upload-core/           独立 Rust 业务库，不依赖 Tauri
  src/model.rs               草稿、附件、发布、任务和结果
  src/store.rs               SQLite 持久化
  src/archive.rs             归档索引、旧版本迁移与内容校验
  src/replay.rs              历史补传预览、去重与计划创建
  src/workspaces.rs          工作区索引、独立引擎及插件范围
  src/lib.rs                 导入、校验、任务调度、取消与重试
  src/drivers/               UploadDriver trait 与本地归档驱动
  src/plugins/               目录扫描、JSON 协议、外部插件进程适配
src-tauri/                   桌面宿主、IPC 命令、文件选择权限
tests/                      前端交互测试
docs/architecture.md         架构、驱动扩展与后续路线
```

```powershell
npm run build
npm run test:ui
cargo test --workspace
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Rust 测试覆盖实际文件归档、上传失败后保留历史、删除源文件后重试、按账号去重、定时与暂停恢复、防止积压突发、归档损坏、旧数据迁移与进程锁。界面测试覆盖预览边界、附件去重、归档独立保存、新账号选择、计划检查与提交、修改配置使预览失效、暂停和取消。测试替身仅用于测试文件，正式界面不注入演示任务。

插件测试启动真实可执行文件，覆盖统一入口上传、历史补传、安装新插件后补传旧历史、校验拒绝、目录与 ID 冲突、超时、取消、崩溃及无效协议响应。插件是以当前用户权限运行的本机程序；不会伪造五个计划平台的远端成功结果。

进一步的设计与实现边界见 [架构说明](docs/architecture.md)，本机测试结果与原生界面验收限制见 [验证记录](docs/validation.md)。
