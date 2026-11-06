# 素酒 · AI Studio 浏览器扩展（实验性）

此目录属于 Sujiu 仓库，保留独立的浏览器适配层，不修改 Rust 核心、HarmonyOS、Android 或 Qt 应用。支持 Chrome / Edge Manifest V3 扩展及 Tampermonkey 油猴脚本，两者从同一份源码构建。

## 功能

- 内嵌 Shadow DOM 提示词面板：系统提示词、角色设定、世界设定、用户人物卡、作者附注，支持本地预设切换。
- 一键尝试填入 AI Studio 原生 System Instructions。若页面结构不匹配，则回退到剪贴板。操作后请检查页面是否保存成功。
- 手动上下文压缩：借鉴 lxchx/aistudio-compressor 的 GenerateContent / ResolveDriveResource / CreatePrompt 适配逻辑。
- 以完整“用户消息 → 模型回复”轮次划分历史，默认保留最近 8 轮，原始聊天保持可访问。
- 创建压缩分支后重新读取并核对消息内容，校验不通过则不自动跳转。

## 安装与验证

需要 Node.js 20+。在 apps/browser-extension 目录运行：

1. npm run check
2. Chrome / Edge 打开 chrome://extensions，启用开发者模式，加载 dist/extension 目录。
3. 使用 Tampermonkey 时导入 dist/sujiu-aistudio.user.js，避免同时启用扩展与油猴版本。
4. 打开 https://aistudio.google.com/prompts/ 下的已有聊天，右下角点击「素酒工具」。
5. 先测试提示词填写；压缩前建议保存原聊天，再测试压缩分支，刷新新会话核对内容。

## 架构

- src/00-state.js：AI Studio 端的状态与配置。
- src/10-settings.js：本地压缩设置、事件桥接。
- src/20-network.js：页面网络事件观察；流式响应不再等待日志解析。
- src/30-compression.js：压缩生命周期与原生发送操作。
- src/35-rounds.js：纯函数，按完整轮次裁剪历史。
- src/40-history.js：读取、解析、组装 AI Studio 内部会话。
- src/50-branching.js：新分支创建及保存后校验。
- src/60-native-ui.js：原有工具栏按钮与页面适配。
- src/70-utils.js：会话记录序列化辅助。
- src/80-prompt-panel.js：素酒提示词侧栏，独立 Shadow DOM。
- src/90-bootstrap.js：初始化。
- scripts/build.mjs：同一源码构建 MV3 和油猴脚本。
- tests/：纯逻辑自动化测试。

这些文件仍处于同一个打包后的 IIFE 作用域，暂时沿用上游的内部结构，避免在首轮移植时同时重写未公开 RPC 协议。模块边界首先按职责划开，后续可以逐步收敛为显式依赖接口。

## 已知限制与安全边界

- Google AI Studio 使用未公开的内部 RPC，索引位置和页面选择器会变。当前无法在用户的登录态浏览器里实测，因此这是**待验收的初版**，请勿当作稳定功能。
- 摘要请求仍通过原会话的原生发送入口发起，因此模型会接触全部历史；提示词要求只总结最后 N 轮之前的内容，但不能保证模型严格遵守。保存到新分支的近期轮次由确定性代码精确保留。
- 分支创建及读取使用 Google 的会话 Cookie 派生认证头及从原生请求捕获的 API Key，数据只在当前页面内存使用，不能写日志、上传第三方服务或纳入预设备份。
- 提示词预设保存在站点 localStorage 中，适合普通设定，不建议写入私密密钥。
- 旧会话不会被删除。压缩操作会向旧会话额外发送一次总结消息；失败时保持旧会话可访问。
- 用户脚本必须以页面上下文运行（@grant none）；Manifest V3 使用 world: MAIN。两者均只匹配 AI Studio 的 prompts 页面。

## 验收场景

准备 60 个已完成轮次，选择保留 8 轮。内部划分结果应为旧 52 轮 + 最近 8 轮，不丢失工具消息。生成的分支要包含摘要消息与近期 8 轮完整记录，并与原模型参数一致。刷新后数据仍在，发出下一次 GenerateContent 请求时旧 52 轮不再以完整消息形式出现。实际摘要质量及旧 52 轮是否被准确覆盖，需要人工检查。

## 致谢与许可证

部分实现改编自 lxchx/aistudio-compressor v0.3.2，原始代码为 MIT 许可。详见 THIRD_PARTY_NOTICES.md；素酒项目本身仍遵循仓库根目录 AGPL-3.0 许可证。
