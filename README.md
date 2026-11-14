# sujiu structured chat for SillyTavern

将一条 `sujiu_schema: "1"` JSON 消息显示为多个说话者的气泡。仍然只使用一张 DM 角色卡，原始聊天消息保持不变。

## 安装

推荐在 SillyTavern 的「安装扩展」中填写 Git URL `https://github.com/Xiwei753/sujiu.git`，并将「Branch or tag name」设为 `sillytavern-structured-chat`。按提示允许第三方扩展、完成安装后刷新酒馆。

也可以下载发行版中的扩展 ZIP，解压后把整个 `sujiu-structured-chat` 文件夹放进 `data/<user>/extensions/`，刷新酒馆。

导入配套 DM 卡与 Chat Completion 预设。角色卡 `data.extensions.sujiu.structured_chat` 为 true 时才启用显示转换。

## 特性

- 一条模型回复包含多个人物的 `segments`，在前台渲染为独立气泡。
- 人物头像使用内置符号或名字首字；可在扩展设置中填写 JSON 映射。
- 支持逐步显示已经生成的片段（浏览器与模型流式协议可能影响具体表现）。
- 可见状态与持有物品，`continuity` 默认折叠并可点击查看。
- 不修改存档的原始结构，不需要为每个人建立角色卡。
- 新版酒馆使用 `MessageFormatter` 钩子；无该钩子的客户端使用 DOM 兼容模式。
- HTML 文本做严格转义，模型输出不会被当作任意脚本执行。

## 测试

运行 `node --test tests/*.test.mjs`。测试覆盖部分/完整 JSON、HTML 转义、渲染器与消息格式化钩子。当前未在用户的 Android 酒馆客户端进行实际端到端测试。

## 协议

`{ "sujiu_schema":"1", "segments":[{"speaker":"御坂美琴","kind":"dialogue","text":"早上好。"}], "state":{"time":"6月16日 08:10","location":"教室","physical":"正常","mental":"平静","inventory":[],"time_marker":"【sujiu:时标】Y0-06-16"}, "continuity":{"nearby":[],"intent":[],"thoughts":[],"plot":"","threads":[],"relations":[],"facts":[]} }`

## 局限

- 提示词要求 JSON，但并非每个 API 都提供可强制的 JSON Schema 模式；模型有可能生成不合格结构。
- 页面只改变显示，不会将 JSON 自动转换成酒馆原生群聊或独立角色卡。
- 旧版兼容模式依赖酒馆的聊天 DOM 结构，可能受第三方客户端界面修改影响。

## 头像说明

本次发行版只内置表情符号和姓名首字头像，**没有捆绑动漫人物的肖像图片**。人物映射可以在扩展设置中改动。
