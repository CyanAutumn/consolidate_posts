# 小红书浏览器发布插件

通过当前项目的 CloakBrowser + Playwright 操作小红书创作者中心，支持图文笔记和单个 MP4 视频，接入现有归档、任务队列、取消与历史补传。尚未使用真实小红书账号发布验证。

## 使用

运行 `run_update.bat` 构建并打开程序，在“平台登录”点击“登录小红书”，完成扫码登录；向导随后打开创作者中心，请完成该页面要求的登录或验证。向导保存 Windows DPAPI 加密的 Cookie，并关闭窗口、重新启动无头浏览器核对账号与创作者会话，通过后才提示成功。不需要复制 Cookie，不保存账号密码。

到发布工作台选择小红书；未启用时先点击卡片启用，再选中发布。标题与正文分别填写。默认无头发布；登录和验证码需本人处理。当前不自动解验证码或处理挑战，不接受 R18 工作区投稿。

当前插件校验上限：标题 20、正文 1000 个 UTF-16 单元；1–18 张 PNG/JPEG/WebP，每图 20 MiB；或单个 MP4，512 MiB。以上是本插件的保守限制，网页及账号规则可能更严格；只验证文件头、大小和种类，不承诺全部视频编码均可发布。不混合图片与视频，不自动截断、转码或拆分笔记。

配置保存在 `<工作区数据目录>/plugin-data/xiaohongshu/<目标ID>/`：`config.json` 为账号标识和发布模式，`profile/` 是独立 Chromium 配置，`session.dpapi.json` 是加密 Cookie，`submissions/` 是各操作的状态和回执。浏览器目录也包含登录信息，需要保护；加密凭据只能由当前电脑的同一 Windows 用户解密。

## 发布与重试

发布前核对当前账号、创作者会话、图片预览数量及发布按钮状态。最终提交只点击一次，并拦截网页再次提交。明确成功响应且返回 24 位十六进制笔记 ID 才写入成功回执；不以离开页面、按钮消失或提示文字单独判断成功。

完成任务再次重试复用回执，不重新打开浏览器。提交前失败可以重试；明确拒绝且没有笔记 ID 可手动重试。提交后断线、超时或没有可靠回执时，保留 `submitting` 并阻止该账号继续提交，先在创作者中心核对。

核对后，确认未发布：先备份记录，再将 `stage` 改为 `prepared`、`receipt` 改为 `null`，重试原任务。确认已发布：先备份，再填写 `stage: "complete"` 和 `receipt: {"location":"https://www.xiaohongshu.com/explore/笔记ID","files":[]}`，原任务重试会读取回执。不要修改原始 `fingerprint` 或 `userId`，不要未经核对清除提交记录。

同账号由原生 Windows 文件锁串行操作。Windows Job Object 在取消插件时终止该插件的 Node/Chromium 子进程，避免取消后仍在后台提交。取消不代表撤回远端笔记。

## 构建与验证

```powershell
npm run plugin:build:xiaohongshu
npm run test:xiaohongshu
& .\plugins\xiaohongshu\plugin.exe --check --data-dir "$env:APPDATA\com.uploadhub.desktop" --target-id xiaohongshu
```

运行需要 Windows x64 和 Node.js 22.12+ / 24。插件分发必须包含本目录固定版本的 Node 依赖，构建脚本会生成 `plugin.exe`；浏览器二进制由 CloakBrowser 下载至用户缓存。首次运行可能需要等待下载。

测试通过真实无头浏览器访问拦截的本地模拟页面，不访问真实账号，也不发布真实笔记。它们验证图文/视频控件、回执解析、账号核对、加密恢复与重复投稿保护，不等于已证明真实平台在线发布可用。页面与提交响应变化时会停止或保留结果未知状态，需要更新适配。

## 调研依据

- [xiaohongshu-mcp](https://github.com/xpzouying/xiaohongshu-mcp)：Apache-2.0，参考创作者会话、图文预览、编辑器及新旧发布控件结构。
- [social-auto-upload](https://github.com/dreammis/social-auto-upload)：MIT，参考图文与视频页面流程。
- [xhs-kit](https://github.com/luweizheng/xhs-kit)：MIT，参考网页表单能力划分。

本插件独立使用项目现有原生启动器和 Node 浏览器方案，不运行上述项目的 Python/Go 服务；未采用循环重复点击发布或仅凭页面跳转判断成功的方式。

[CloakBrowser](https://github.com/CloakHQ/CloakBrowser) 包装代码与浏览器二进制采用不同许可；二进制从上游缓存下载，不随本项目重新分发。页面生成授权与上传请求，插件没有内置私有 API 签名、授权令牌或账号密码。
