# 插件模板

这是可编译运行的最小插件。默认禁用，示例只把附件和清单导出到本地，不执行网络投稿。

```powershell
npm run plugin:new -- my-platform "我的平台"
.\plugins\my-platform\build-plugin.ps1
```

统一入口为 `plugin.exe --upload-hub-plugin`，Rust 使用 `run_plugin(YourPlugin)`。

- `validate(publication)`：纯内容校验，不登录、不发布、不修改远端。
- `upload(publication, context)`：核对账号配置，流式读取归档文件，报告进度，确认远端成功后返回 `Receipt`。
- `context.workspace.data_dir / context.target.id`：本工作区、本账号的配置目录。凭据不放入源码、日志、任务或归档。
- `context.job_id`：当前尝试 ID；`context.operation_id`：跨重试稳定的原始操作 ID，用于投稿日志和幂等处理。新的补传操作有新的 ID。
- `context.check_cancelled()`：检查取消。主程序也会在取消和超时时终止整个插件进程，因此先持久化远端提交状态。

stdout 只允许 SDK 的 JSON 协议。诊断信息使用 stderr，并避免输出凭据。`workspace` 在旧协议请求中可能为空，模板仍支持本地导出；需要账号隔离的真实平台应明确拒绝缺少上下文的请求。

同目录 `plugin.json` 声明能力与可执行入口。复制模板后，构建脚本会读取 Cargo 包名并生成 `plugin.exe`。完整协议见 [插件开发说明](../README.md)，实际图片上传实现见 [Pixiv 插件](../pixiv/README.md)。
