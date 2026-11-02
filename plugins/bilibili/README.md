# 哔哩哔哩浏览器投稿插件

使用项目现有 CloakBrowser + Playwright 浏览器运行时，在 B 站创作中心上传并提交单个 MP4 视频。已接入平台登录、工作区、归档、队列、历史补传、取消和投稿日志。尚未使用真实 B 站账号投稿验证。

## 使用

运行 `run_update.bat` 打开更新后的程序。在“平台登录”点击“登录哔哩哔哩”，由本人完成扫码或账号登录和验证码。程序使用官方只读登录状态响应核对数字用户 ID，保存 DPAPI 加密 Cookie，再重新启动无头浏览器确认登录态可用。

展开“投稿设置”，填写：

- 分区路径：按当前网页显示的父分区/子分区名称填写，以 `/` 分隔；程序按文字逐级选择，不内置可能过期的分区目录。
- 标签：1–10 个，逗号分隔，每个最多 20 个 UTF-16 单元。
- 创作类型：明确选择自制或转载；转载必须填写来源。

点击“保存B站投稿设置”。设置与登录独立保存，不需要重新登录。程序不猜测创作类型、分区或转载来源。

在工作台填写标题与简介，导入一个 MP4，启用并选中哔哩哔哩后发布。当前插件上限为单视频 2 GiB，标题 80、简介 2000 个 UTF-16 单元；实际视频编码、账号权限和网页要求仍由 B 站校验。本版不支持图片投稿、多 P、自定义封面或付费投稿；封面使用网页自动生成结果，网页不接受时会停止。R18 工作区不接受投稿。验证码需要本人处理，不会自动解决。

## 登录态与数据

`<工作区目录>/plugin-data/bilibili/<目标ID>/` 包含：

- `config.json`：数字用户 ID、昵称和发布模式。
- `posting.json`：分区、标签、创作类型与来源。
- `session.dpapi.json`：Windows DPAPI 加密 Cookie，只能由当前电脑的同一 Windows 用户解密。
- `profile/`：独立浏览器配置，也包含登录信息，需要妥善保护。
- `submissions/`：跨重试保持一致的投稿操作记录。

凭据经子进程私有管道传给系统加密实现，不写入源码、归档、任务数据库或日志。不同工作区与账号目录隔离。

## 投稿确认与重试

网页负责授权、视频上传、封面及投稿请求。提交前核对当前账号、最终请求的标题、简介、标签、自制/转载、来源、分区编号及单个已上传视频；出现不一致会拦截请求。投稿按钮只点击一次，再次提交被阻止。

只有明确成功响应和有效 BV/AV 号才能生成回执；页面跳转或“成功”文字本身不算确认。完成任务重试直接读取回执，不再次投稿。明确拒绝且没有稿件编号时可手动重试。点击后断线、超时或没有可靠回执，保留 `submitting`，并阻止该账号继续投稿，先到稿件管理核对。

确认未投稿：先备份对应日志，再将 `stage` 改为 `prepared`、`receipt` 改为 `null`，重试原任务。确认已投稿：先备份，再将 `stage` 改为 `complete`，填写 `receipt: {"location":"https://www.bilibili.com/video/BV编号","files":[]}`，原任务重试会读取回执。不要修改 `fingerprint` 或 `userId`，不要未经核对清除记录。

同账号由原生 Windows 文件锁串行执行；取消插件时，Windows Job Object 清理其 Node/Chromium 子进程。取消本地任务不代表撤回远端稿件。后台视频转码或审核尚未结束也可能已经取得投稿回执，回执代表已提交稿件，不代表已通过审核。

## 构建与测试

```powershell
npm run plugin:build:bilibili
npm run test:bilibili
& .\plugins\bilibili\plugin.exe --check --data-dir "$env:APPDATA\com.uploadhub.desktop" --target-id bilibili
```

需要 Windows x64、Node.js 22.12+ / 24。必须随插件包含固定版本的 Node 依赖；CloakBrowser 浏览器二进制由上游下载到用户缓存，不随项目分发。

测试在真实无头浏览器中拦截所有网络请求，用模拟网页验证视频选择、标题/简介、分类、标签、转载来源、请求校验、回执、失败恢复及加密登录态。测试不会登录真实账号或投稿；在线页面变化仍可能需要更新选择器和响应适配。

## 开源调研

- [multi-publisher 的 B 站浏览器适配](https://github.com/atxinsky/multi-publisher/blob/main/src/worker/platforms/bilibili.ts)：MIT，参考创作中心地址及上传、标题、简介和标签控件。
- [biliup](https://github.com/biliup/biliup) 与 [biliup-rs](https://github.com/biliup/biliup-rs)：参考投稿元数据、稿件标识及上传能力；没有引入额外 CLI 或其登录服务。
- [social-auto-upload B 站运行时](https://github.com/dreammis/social-auto-upload/blob/main/uploader/bilibili_uploader/runtime.py)：MIT，参考专用上传器方案与当前项目结构的取舍。

本插件使用现有原生启动器和 Node 浏览器方案独立实现，不采用仅依据页面跳转判断成功，也不循环点击投稿。[CloakBrowser](https://github.com/CloakHQ/CloakBrowser) 包装代码和浏览器二进制有不同许可，浏览器从上游下载到用户缓存。
