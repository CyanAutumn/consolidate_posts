# X (Twitter) 浏览器发布插件

通过 **CloakBrowser + Playwright** 操作 X 网页，复用 Upload Hub 的归档、工作区、多个账号、队列、取消与历史定时补传。不需要 X 开发者 API 密钥。支持纯文字、最多四张 PNG / JPEG / WebP、单个 GIF 或单个 MP4。

已通过真实无头浏览器操作本地模拟页面，以及原生插件协议、进程树取消测试；**尚未使用真实 X 账号发布验证**。X 页面结构、验证或接口变化可能需要更新插件。

## 使用

1. 双击项目根目录 `run_update.bat`，构建会同时准备本插件。也可以单独运行 `npm run plugin:build:x`。
2. 在“工作区管理”中启用 **X (Twitter)**，然后到“平台登录”找到 X。
3. 点击“登录”，在打开的独立浏览器中完成登录和验证，识别账号后自动保存并关闭窗口。每个平台显示一个账号，不需要复制 Cookie 或填写用户名。首次启动可能下载浏览器组件。
4. 在主程序填写文字、导入媒体、勾选该账号并发布。成功回执为 `https://x.com/<用户名>/status/<帖文ID>`；历史补传使用同一入口。

```powershell
# 单独准备插件及验证
npm run plugin:build:x
npm run test:x

# 配置示例：DataDir 使用主程序显示的“当前工作区数据目录”
& .\plugins\x\configure.ps1 -DataDir 'C:\实际工作区数据目录' -TargetId 'x' -Username 'your_handle'

# 默认发布时不显示窗口；需要查看时，在配置命令追加 -Headed
# 登录始终显示窗口，发布模式只影响后续上传
```

需要 Node.js 22.12+ / 24、Windows x64。源码构建还需要 Rust MSVC；已编译插件运行时不需要 Rust。插件目录内的 `node_modules/` 是必要运行依赖，构建分发时会一起复制；不要只复制 `plugin.exe`。CloakBrowser 的 Chromium 二进制使用用户缓存，不打包进项目。

若 PowerShell 的执行策略阻止 `.ps1`，可只对本次命令使用 `powershell -ExecutionPolicy Bypass -File <configure.ps1的路径> -DataDir ... -TargetId ...`。不需要修改系统执行策略。

## 登录信息保存位置

```text
<当前工作区数据目录>/plugin-data/x/<目标ID>/
  config.json       用户名、数字用户ID、发布窗口及敏感标记默认值
  profile/          独立 Chromium 配置、Cookie、localStorage、缓存
  browser.lock      Windows 文件互斥锁（进程退出自动释放，不需删除）
  submissions/      每个 operationId 的发布状态及已确认回执
```

`config.json` 示例：

```json
{"username":"your_handle","userId":"123456789","headless":true,"sensitive":false}
```

Cookie 由 Chromium 自己管理，不写进 JSON、任务数据库或作品归档。`profile/` 包含账号登录状态，应和其他账号私有数据一样保护；浏览器配置目录并非整目录加密，跨电脑迁移后可能需要重新登录。不同工作区、不同目标各自保存配置，发布前检查登录用户 ID 与用户名，防止串号。更换 X 账号时请新建目标。

登录失效、账号验证或网页挑战需要重新运行配置命令，由本人在浏览器处理。插件不会自动解验证码。使用系统网络 / 代理设置；没有自定义 GraphQL 服务地址，也没有浏览器远程调试端口对外开放功能。

## 内容规则

- 标题和正文去除首尾空白后以一个空行合并。使用 X 官方开源 `twitter-text` 的 280 加权字符规则，包括中文、emoji、链接；此版不发长帖或自动拆成线程，不静默截断内容。
- 普通图片最多四张，每张 5 MiB；GIF 只能单独发，最大 15 MiB；MP4 只能单独发，本插件先限制为 512 MiB。其他附件和混合视频 / 图片会在发布前拒绝。
- 本地校验检查实际文件头、文件类型、大小和数量；不对视频转码或检测全部编码参数。解码、时长、尺寸、帧数和账号权限仍由 X 网页检查，浏览器接受附件不保证远端发布成功。
- R18 工作区强制为网页最终发布请求设置 `possibly_sensitive: true`；其他工作区可以在配置中设置 `sensitive: true`。如果网页不再提供该字段，停止提交。此标记不是年龄验证，也不会改变账号公开 / 受保护状态。
- 页面负责生成上传请求、GraphQL 查询 ID 和登录授权；插件操作编辑器、文件选择和发布按钮，只核对最终发出的文字 / 媒体数量并接收结果。没有把当前私有查询 ID、授权 token 或登录密码写死在代码中。

## 中断与重试

每次点击发布前，先将 `operationId` 的记录写成 `submitting`，收到数字帖文 ID、匹配账号和完整媒体回执后保存 `complete`。同一操作重试若已成功，直接复用回执，不再次打开浏览器发布。

如果点击之后断线或进程被取消，可能已经发布但没有收到回执。此时保留 `submitting`，阻止同一账号继续发新帖或盲目重试。先打开个人主页核对，然后：

- **确认未发布**：备份该记录，将 `stage` 改为 `prepared`、`receipt` 改为 `null`，再重试原任务。
- **确认已发布**：备份后将 `stage` 改为 `complete`，填写 `receipt: {"location":"https://x.com/你的用户名/status/数字帖文ID","files":[]}`；重试原任务会读取该回执。不要修改 `fingerprint` 或 `userId`。

这一步由本人核对，插件不会猜测远端成功。明确的 HTTP 拒绝（例如 401 / 403 / 429）可在处理原因后重试。取消本地任务不会撤回已发送的帖子。

同账号最多一个登录 / 发布进程，其他任务最多等待十分钟。Windows Job Object 确保主程序取消插件时，一并关闭所属 Node 和 Chromium 子进程，不留后台浏览器继续提交。定时发布仍要求主程序开启。

## 开源调研与依赖

调研了 [Twikit](https://github.com/d60/twikit)（Python 私有 API 客户端）和 [XActions](https://github.com/nirholas/XActions)（自动化工具与网页脚本）。最终针对当前项目独立实现发布流程，使用用户指定的 [CloakBrowser](https://github.com/CloakHQ/CloakBrowser) 的 Node 包和 [Playwright](https://github.com/microsoft/playwright)，不引入另一套 Python 服务或整套社交自动化工具。

依赖版本固定于本目录 `package-lock.json`。CloakBrowser 包装代码采用 MIT，浏览器二进制有独立的[使用与分发许可](https://github.com/CloakHQ/CloakBrowser/blob/main/BINARY-LICENSE.md)，因此只从上游下载到用户缓存；较新二进制 / 并发许可按上游政策处理，本插件不要求付费 key。[twitter-text](https://github.com/twitter/twitter-text) 用于文字校验，第三方许可保留在随附依赖中。

测试命令只操作临时本地模拟页面，不登录、不访问真实 X 内容、不发真实帖子。它们证明本地流程和错误处理正常，不代表所有真实账号都能通过平台检查。

可通过原生入口只检查已保存的登录状态（不会发帖）：

```powershell
& .\plugins\x\plugin.exe --check --data-dir "$env:APPDATA\com.uploadhub.desktop" --target-id x
```

如果主程序沿用了旧目标 ID，替换 `--target-id` 为该 ID。检查复用独立浏览器配置，以无头方式核对账号并关闭浏览器。
