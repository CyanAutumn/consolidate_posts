// Separate from publishing so the first binary download never consumes a job's timeout.
process.env.CLOAKBROWSER_AUTO_UPDATE ??= "false";
const { ensureBinary } = await import("cloakbrowser");
await ensureBinary();
console.log("X 浏览器组件已就绪。");
