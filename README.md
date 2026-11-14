# sujiu structured chat for SillyTavern

将一条 `sujiu_schema: "1"` JSON 消息显示为多个说话者的气泡。仍然只使用一张 DM 角色卡，原始聊天消息保持不变。

## 安装

推荐在 SillyTavern 的「安装扩展」中填写 Git URL `https://github.com/Xiwei753/sujiu.git`，并将「Branch or tag name」设为 `sillytavern-structured-chat`。按提示允许第三方扩展、完成安装后刷新酒馆。

也可以下载发行版中的扩展 ZIP，解压后把整个 `sujiu-structured-chat` 文件夹放进 `data/<user>/extensions/`，刷新酒馆。

导入配套 DM 卡与 Chat Completion 预设。扩展启用后，能识别的结构化 JSON 会自动渲染；即使角色卡没有 `data.extensions.sujiu.structured_chat` 标记也能识别。默认也尝试识别有明确角色标签的普通文本（例如独立一行的“同学A”“老师”、带冒号的台词）。无法确定说话人的文本继续按原样显示。

## 特性

- 一条模型回复包含多个人物的 `segments`，在前台渲染为独立气泡；普通文本有明确角色标签时也能兼容渲染。
- 人物头像使用内置符号或名字首字；可在扩展设置中填写 JSON 映射。
- 支持 JSON 片段的原生格式化钩子流式显示；DOM 兼容模式只接管已存档的完成消息，以免覆盖正在生成的文字。
- 可见状态与持有物品，`continuity` 默认折叠并可点击查看。
- 不修改存档的原始结构，不需要为每个人建立角色卡。
- 新版酒馆使用 `MessageFormatter` 钩子；无该钩子的客户端使用 DOM 兼容模式。
- HTML 文本做严格转义，模型输出不会被当作任意脚本执行。

## 测试

运行 `node --test tests/*.test.mjs`。测试覆盖部分/完整 JSON、HTML 转义、渲染器与消息格式化钩子。已添加与 Android 截图对应的三人物文本回归用例；仍需在 Android 酒馆客户端完成真实端到端验证。

## 协议

`{ "sujiu_schema":"1", "segments":[{"speaker":"御坂美琴","kind":"dialogue","text":"早上好。"}], "state":{"time":"6月16日 08:10","location":"教室","physical":"正常","mental":"平静","inventory":[],"time_marker":"【sujiu:时标】Y0-06-16"}, "continuity":{"nearby":[],"intent":[],"thoughts":[],"plot":"","threads":[],"relations":[],"facts":[]} }`

## v4.0.2 修复与排查

- 已根据 SillyTavern 官方 `getContext()` 源码更正事件字段为 `eventTypes`，修复聊天切换、新消息等事件未注册的问题。支持旧客户端的 `event_types` 兼容字段。


- 版本：安装分支根目录的 `manifest.json` 标记为 `4.0.2`。通过 Git 安装的扩展可在酒馆扩展管理中执行更新，然后刷新页面。
- 在扩展设置中展开「sujiu · 多角色气泡」，可看到当前是「消息格式化 + DOM 兼容」还是「DOM 兼容」模式、角色卡标记与最近识别结果。
- 「兼容识别普通文本中的角色台词」默认开启，可随时关闭。纯文本需要明确的角色标签；无法识别时保持原文，且**不会修改聊天存档**。
- 如只显示普通文本，先查看诊断。如果扩展已加载却显示「原文」，检查模型是否输出包含 `sujiu_schema` 和 `segments` 的 JSON，或者使用 `老师：台词`、单行角色名 + 下一行台词的明确格式。
- 旧的 JSON 存档无需重新导入；角色卡、世界书、私有 DM 配置不随此扩展发布。

## 局限

- 提示词要求 JSON，但并非每个 API 都提供可强制的 JSON Schema 模式；模型可能生成普通文本。v4.0.2 提供保守的文本兼容解析，不猜测未知说话人。
- 页面只改变显示，不会将 JSON 自动转换成酒馆原生群聊或独立角色卡。
- 旧版兼容模式依赖酒馆的聊天 DOM 结构，可能受第三方客户端界面修改影响。

## 头像说明

本次发行版只内置表情符号和姓名首字头像，**没有捆绑动漫人物的肖像图片**。人物映射可以在扩展设置中改动。
