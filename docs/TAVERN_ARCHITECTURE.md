# Sujiu 酒馆整体架构规范

> 状态：目标架构 / 上位设计规范  
> 适用范围：AI 对话、角色扮演、Prompt、世界书、Tool Loop、Provider / Protocol 接入、跨平台边界  
> 说明：`docs/ARCHITECTURE.md` 继续记录当前实现细节；当当前实现与本文冲突时，本文代表目标路线，冲突项应作为迁移任务处理，而不是继续扩大旧抽象。

## 1. 核心判断

Sujiu 不需要成为一个“高度抽象的通用 LLM Framework”。

它只需要成为：

> 一套稳定的酒馆领域模型 + 一个确定性的 Prompt 组装器 + 一个标准 Tool Loop + 若干薄的协议 Adapter + 三套原生 UI。

酒馆的主链必须始终保持简单：

```text
用户选择会话
    ↓
会话引用角色 / Persona / 世界书 / 提示词
    ↓
根据当前历史和用户输入组装 Prompt
    ↓
选择已经解析好的模型 Route
    ↓
发送模型
    ↓
如果模型调用工具，则执行工具并继续本轮
    ↓
得到最终回复
    ↓
完整保存 Turn
    ↓
UI 按自己的方式展示
```

以后所有架构判断都围绕这条主链。

不要再为了某一个 Provider、某一种缓存、某一个 UI、未来可能存在的群聊，或者某个第三方协议的小特性，改变这条主链。

---

## 2. 当前路线：保留什么，纠正什么

### 2.1 保留

以下方向正确，不推翻：

- Conversation 是会话根实体。
- Character、Persona、WorldBook、PromptProfile 是独立资源。
- Conversation 只保存这些资源的引用。
- UI 展示历史与模型真实 Transcript 分离。
- Tool Call / Tool Result 必须完整保存。
- 平台 UI 不自行拼 Prompt。
- 平台 UI 不自行处理 Provider。
- 用户只配置 Base URL、API Key、模型。
- 不要求用户选择“OpenAI / DeepSeek / Anthropic”等供应商。
- Responses 优先于 Chat Completions。
- Chat Completions 是最重要的兼容兜底。
- compaction 属于共享 Runtime。
- API Key 留在平台安全存储中。

### 2.2 需要纠正

当前最大的路线问题不是某个具体函数，而是**把技术细节提升成了领域抽象**。

需要纠正五点。

#### 第一：Prompt 不应以缓存结构为中心

Prompt 的第一职责只有一个：

> 准确表达这次模型究竟应该看到什么，以及先后顺序。

目标结构：

```text
PromptAssembly
    ordered_items
    dropped_items
    size_estimate
```

缓存分析只能发生在编译完成之后：

```text
CompiledPrompt
    ↓
CacheAnalyzer
    ↓
CacheReport
```

缓存绝不能反过来决定 WorldBook 应放哪、Persona 应放哪、历史如何表达。

#### 第二：Wire Protocol 不应属于 Core Domain

`sujiu-core` 应只认识领域语义：

```text
Conversation
Character
Persona
WorldBook
PromptProfile
Transcript
Turn
ModelMessage
ToolCall / ToolResult
```

它不应该知道：

```text
OpenAI Responses
Chat Completions
Anthropic Messages
/responses
/chat/completions
/messages
```

协议、HTTP 路径、Endpoint Probe、Model Listing、Provider Capability 应属于 AI 接入层。

Core 的数据在完全没有 HTTP 的情况下也必须正常工作。

#### 第三：Protocol 是“模型 Route”的属性，不只是 Endpoint 的属性

不能默认同一 Endpoint 下所有模型一定共享相同能力。

最终调用对象应明确到：

```text
ResolvedModelRoute {
    endpoint_id
    model_id
    protocol
    capabilities
}
```

缓存键至少包含：

```text
endpoint + model
```

Model Discovery 只回答：

> 这里有什么模型？

Route Resolver 回答：

> 这个具体模型到底应该怎么调用？

两者不能混在一起。

#### 第四：`previous_response_id` 不应成为持久化会话模型的一部分

既然当前约束已经明确：

```text
Responses continuation
只在同一次用户 Turn 的工具循环内使用
不跨用户 Turn
```

那么它就是执行态，而不是长期 Conversation 数据。

正确关系：

```text
Persistent Transcript
    用户消息
    assistant 内容
    tool call
    tool result
    必要的可重放 provider sidecar

Transient Turn State
    previous_response_id
    当前 response lineage
    当前 round
    当前正在等待的工具
```

一次用户 Turn 完成后，`previous_response_id` 应自然死亡。

#### 第五：尚未实现的群聊 / 跑团能力不能决定 V1 架构

Conversation 可以继续保存：

```text
participants[]
```

因为数据结构本身合理，也避免以后迁移。

但 V1 主执行路径只需要明确：

```text
0 个角色
1 个角色
```

`2+ participants` 可以被存储和展示，但在真正设计 speaking policy 前：

- Core 不猜谁说话。
- Runtime 不自动轮换。
- 不因为未来跑团提前加入复杂调度器。
- 不为了 Narrator / Group Chat 改造普通单角色 Prompt。

数据结构可以提前兼容未来，执行抽象不要提前实现未来。

---

## 3. 唯一主数据流

以后所有发送请求必须走：

```text
Conversation
    +
Resource Library
    +
Current User Input
        ↓
ContextResolver
        ↓
ResolvedConversationContext
        ↓
PromptAssembler
        ↓
CompiledPrompt
        ↓
ModelRequest
        ↓
ResolvedModelRoute
        ↓
ProtocolAdapter
        ↓
AgentLoop
        ↓
Turn
        ↓
Transcript
        ↓
Persistence
        ↓
UI Projection
```

任何平台不得绕过这条链。

不存在：

```text
HarmonyOSPromptBuilder
AndroidPromptBuilder
DesktopPromptBuilder
```

也不存在：

```text
ResponsesTranscript
ChatTranscript
```

只有一份会话、一份 Transcript、一套 Prompt 语义。

---

## 4. 领域实体

### Conversation

Conversation 表示一次独立会话：

```text
Conversation {
    id
    title

    participants
    persona_id
    worldbook_ids
    prompt_profile_id

    transcript

    metadata
}
```

Conversation 保存引用，不复制资源正文。

### Character

V1 至少保留：

```text
id
name
description
personality
scenario
first_message
alternate_greetings
example_dialogue
system_prompt
default_worldbook_ids
extensions
```

Character 不拥有 WorldBook。

删除 Character 也不能删除 WorldBook。

### Persona

Persona 表示“用户在这个故事中是谁”：

```text
id
name
description
user_prompt
default_worldbook_ids
extensions
```

Persona 是独立资源，不能塞进 Character。

### WorldBook

WorldBook 是独立 Lore 数据：

```text
WorldBook
    id
    name
    entries[]
```

V1 Entry：

```text
WorldBookEntry {
    id
    enabled

    keys[]
    constant

    content

    order
    position

    extensions
}
```

先实现成熟酒馆最常用的能力：

```text
constant
keyword match
order
position
```

不要第一版就实现一整个规则语言。

以后真正需要时再增加：

```text
AND / NOT
递归触发
概率
复杂扫描深度
额外过滤器
```

### PromptProfile

PromptProfile 是一套可复用 Prompt 配置。

V1 应限制为明确的几个位置：

```text
system_prompt
format_rules
post_history_instructions
extra_segments[]
```

额外段只允许有限位置：

```text
Prefix
BeforeHistory
PostHistory
```

不要把 PromptProfile 做成任意图结构或脚本系统。

第三方导入格式存在更多位置时，由 Import Adapter 映射。

---

## 5. WorldBook 绑定与激活

### 5.1 绑定来源

资料来源按以下范围解析：

```text
Conversation WorldBook
Persona default WorldBook
Character default WorldBook
Global WorldBook
```

同一个 WorldBook ID 只加载一次。

WorldBook 的来源只决定绑定和冲突处理。

最终真正进入 Prompt 时，按 Entry 的：

```text
position
order
```

统一排序。

不能因为它来自角色还是 Persona 就偷偷改变内容本身的语义。

### 5.2 激活

每次生成前构造扫描文本：

```text
最近有效历史
+
当前用户输入
+
必要时 compacted summary
```

V1 规则：

```text
enabled == false
    → 不激活

constant == true
    → 激活

否则：
    keys 中任一关键词命中
    → 激活
```

然后：

```text
按 position 分组
按 order 排序
应用上下文预算
```

被预算裁掉的条目必须进入诊断，但不能删除资源本身。

---

## 6. Prompt 的唯一标准顺序

默认 Chat Prompt 按以下顺序组装：

```text
1. Main / Profile System Prompt

2. WorldBook: Before Character

3. Persona Description

4. Character Definition
   - name
   - description
   - personality
   - scenario
   - character system instruction

5. WorldBook: After Character

6. Example Dialogue

7. Compacted Summary（如果存在）

8. Chat History
   - user
   - assistant
   - tool call
   - tool result

9. Near-History / depth injection

10. Post-History Instructions

11. Current User Message
```

Prompt 顺序首先服从语义。

不为了缓存命中率移动条目。

---

## 7. PromptAssembler

PromptAssembler 输入：

```text
ResolvedConversationContext {
    character(s)
    persona
    worldbooks
    prompt_profile
    transcript
    current_user_input
}
```

输出：

```text
CompiledPrompt {
    messages[]
    activated_lore[]
    dropped_lore[]
    estimated_size
}
```

内部可保留来源信息：

```text
PromptItem {
    role
    content
    source
}
```

但发送给模型前最终只形成统一：

```text
ModelMessage[]
```

Provider 不认识：

```text
Character
Persona
WorldBook
PromptProfile
```

它只认识已经编译好的 ModelRequest。

---

## 8. Transcript

Transcript 是长期真相。

不能为了 UI 好看而删除工具过程。

一次用户 Turn：

```text
Turn
├── UserMessage
├── AssistantStep
│   └── ToolCall + ToolResult
├── AssistantStep
│   └── ToolCall + ToolResult
└── AssistantStep
    └── Final Text
```

下一轮模型看到的历史仍然包含整个过程。

UI 可以只显示：

```text
用户
最终回答
```

并把工具过程折叠。

但是 UI Projection 和 Transcript 必须是两回事。

---

## 9. Tool Loop

工具系统不负责角色资料、Persona 或普通 WorldBook。

这些都直接编入 Prompt。

工具只用于真正需要运行时取数据的内容，例如：

```text
外部文件
巨大知识库
网络搜索
MCP
长期记忆查询
大型历史检索
其他外部能力
```

标准循环：

```text
发送模型
    ↓
assistant stream
    ↓
没有 tool call
    → Turn 完成

有 tool call
    ↓
执行工具
    ↓
保存 call + result
    ↓
把结果继续交给当前模型
    ↓
下一 round
```

工具循环属于当前用户 Turn。

---

## 10. Endpoint、Model Discovery 与 Route Resolution

### 10.1 用户配置

用户只提供：

```text
Base URL
API Key
```

然后：

```text
探测模型
→ 用户选择模型
→ 解析该模型的 Route
→ 保存成功结果
```

用户不需要选择：

```text
OpenAI
DeepSeek
Anthropic
OpenRouter
```

这些最多只是 UI 友好名称，不能影响协议选择。

### 10.2 Model Discovery

只负责：

> 这个 Endpoint 能列出哪些模型？

模型列表拿不到：

```text
≠ Endpoint 不可用
```

必须允许用户手输模型名。

### 10.3 Route Resolution

针对：

```text
endpoint + model
```

解析真正可用 Route。

当前优先顺序：

```text
1. OpenAI Responses
2. OpenAI Chat Completions
3. Anthropic Messages
```

但第三项只有实现完整 Adapter 后才允许进入可运行结果。

没有 Adapter：

```text
可以识别
不能宣称支持
```

成功结果：

```text
ResolvedModelRoute {
    endpoint_id
    model_id
    protocol
    capabilities
}
```

正常 Turn 不再每次从头猜 Provider。

配置变化、模型变化或 Route 明确失效时重新解析。

---

## 11. Protocol Adapter

Adapter 只干最后一公里。

例如：

```text
OpenAiResponsesAdapter
OpenAiChatAdapter
AnthropicMessagesAdapter
```

负责：

```text
ModelMessage → wire JSON
ToolDefinition → wire tool schema
stream parser
usage parser
reasoning field
protocol-native continuation
```

Adapter 不负责：

```text
选择角色
匹配世界书
裁剪历史
决定 Persona
拼 System Prompt
管理 Conversation
决定 UI 状态
```

Responses 和 Chat 不是两套业务逻辑。

它们只是同一个：

```text
ModelRequest
```

经过两种 Adapter。

---

## 12. Continuation 与 Reasoning

### 12.1 Continuation

OpenAI Responses 的 `previous_response_id` 只存在于：

```text
TurnExecutionState
```

例如：

```text
TurnExecutionState {
    round
    current_route
    continuation
    cancellation
}
```

作用范围：

```text
用户发送 U1
    ↓
round 1
    ↓
tool
    ↓
round 2
    ↓
tool
    ↓
round 3 final
    ↓
TurnExecutionState 销毁
```

下一次用户发送 U2，重新以持久化 Transcript 为基础。

V1 不做跨用户 Turn continuation。

### 12.2 Reasoning

Reasoning 分两种。

#### Provider-native opaque state

例如只能通过 Responses lineage 正确延续的状态。

只在当前 Turn 内存在。

不要伪造，不要跨 Provider。

#### Endpoint 要求 replay 的 reasoning sidecar

部分 OpenAI-compatible Chat Endpoint 会要求把：

```text
reasoning_content
```

跟历史 assistant message 一起发回。

这种数据可以作为：

```text
ProviderReplaySidecar
```

跟对应 AssistantStep 保存。

但必须带：

```text
endpoint/model/protocol identity
```

身份不匹配则不 replay。

可见 assistant text 永远正常保留。

---

## 13. Prompt Cache

缓存不是业务状态。

业务层只保证：

```text
稳定内容尽量稳定
旧历史不无意义重写
Tool 定义顺序稳定
新消息正常追加
```

然后记录 Provider 返回的：

```text
input_tokens
cached_input_tokens
cache_write_tokens
output_tokens
```

是否真正命中缓存，只认 Provider usage。

不要根据：

```text
HTTP body 看起来短
```

推断便宜。

也不要根据：

```text
previous_response_id
```

推断旧历史不计费。

如果需要调试缓存，保留独立工具：

```text
CacheAnalyzer::compare(previous, current)
```

输出例如：

```text
same_prefix_messages
changed_at
possible_reason
```

它只用于：

```text
诊断
测试
性能分析
```

不能进入 Prompt 的核心领域模型。

---

## 14. Compaction

Compaction 属于 Runtime。

触发前：

```text
Transcript
→ 判断预算
```

需要压缩时：

```text
previous_summary
+
newly_archived_turns
    ↓
summarizer
    ↓
new_summary
```

绝不再次发送：

```text
已经被 previous_summary 总结过的全部 archived raw turns
```

原始历史继续保存在存储中：

```text
供 UI 回看
供精确检索
```

但不再持续进入模型 Prompt。

切割必须按完整 Turn。

Tool Call / Tool Result 不允许被切开。

---

## 15. Context / Retrieval

Character、Persona、WorldBook 的真实数据仍在 Library。

如果为了搜索把它们投影进 Context Index：

```text
Context Index
```

只能是派生索引。

绝不能成为第二份真相。

关系必须永远是：

```text
Resource
    ↓ derive
Context Index
```

不能双向编辑。

---

## 16. Runtime / FFI / UI 边界

Rust Runtime 向平台提供粗粒度操作：

```text
create_conversation
update_conversation_bindings

list_characters
save_character

list_personas
save_persona

list_worldbooks
save_worldbook

list_prompt_profiles
save_prompt_profile

configure_endpoint
discover_models
select_model

send_turn
cancel_turn

conversation_snapshot
```

UI 不应该拿到一堆 Core 小函数以后自己重新组织业务。

Android / HarmonyOS / Desktop 自己维护：

```text
当前页面
Drawer
Sheet
焦点
滚动位置
动画
输入框
窗口
平台权限
文件选择
安全存储
```

Rust 不管理这些。

Rust 返回：

```text
ConversationSnapshot
LibrarySnapshot
ModelList
StreamEvent
TurnState
Diagnostics
```

---

## 17. 资料库、会话和设置必须分开

```text
资料库
├── Character
├── Persona
├── WorldBook
└── PromptProfile
```

资料库回答：

> 我拥有哪些资料？

Conversation Contents 回答：

> 这个会话使用哪些资料？

Settings 回答：

> App 和 AI 服务怎么配置？

设置页不得再次编辑角色 / Persona / 世界书正文。

---

## 18. V1 明确不做的事

为了避免再次抽象失控，以下能力不得影响 V1 主路线：

- 自动群聊发言调度。
- 跑团主持人策略。
- 完整 SillyTavern Script。
- 任意 Prompt 图。
- 每个 Provider 一套会话模型。
- 跨用户 Turn Responses continuation。
- 根据模型名称猜供应商。
- 根据供应商名称猜协议。
- 让模型自己判断 WorldBook 是否相关。
- 为了缓存移动 Prompt 的语义位置。
- UI 自己拼上下文。
- 三个平台各实现一套 AI 请求链。

以后需要哪项，再基于真实需求增加。

---

## 19. 推荐代码职责

不要为了架构漂亮继续拆 crate。

### `sujiu-core`

只放：

```text
conversation
character
persona
worldbook
prompt_profile
prompt assembly
transcript
model-neutral messages
tool-neutral data
```

逐步移出：

```text
OpenAI / Anthropic protocol
HTTP endpoint probe
model listing protocol
wire capability
```

### `sujiu-ai`

负责：

```text
endpoint
model discovery
model route
protocol negotiation
Responses adapter
Chat adapter
Anthropic adapter
stream parser
tool execution loop
usage
provider diagnostics
```

### `sujiu-runtime`

负责把所有东西串起来：

```text
storage
library
conversation bindings
prompt assembly
compaction
route resolution
agent execution
persistence
events
FFI-facing application API
```

---

## 20. 测试策略

### Prompt Golden Test

固定：

```text
Character
Persona
WorldBook
PromptProfile
Transcript
User Input
```

断言最终：

```text
ModelMessage[]
```

完全一致。

这是最重要的测试。

### WorldBook Test

覆盖：

```text
constant
keyword
disabled
order
position
预算裁剪
重复绑定
```

### Adapter Wire Test

同一个：

```text
ModelRequest
```

分别经过：

```text
Responses
Chat Completions
Anthropic
```

检查真实 HTTP body。

### Agent Loop Test

覆盖：

```text
普通回答
1 次工具
连续 3 次工具
tool error
cancel
max rounds
provider error
```

确保 Transcript 完整。

### Persistence Test

关闭应用重新打开后，以下内容都必须恢复：

```text
资源
Conversation bindings
Transcript
Compaction summary
Provider config
```

Transient continuation 不要求恢复。

---

## 21. 重构顺序

不要同时开多个会修改同一批底层文件的并行议题。

### 第一阶段：架构收口

```text
1. 用本文档固定上位路线
2. 把 Protocol / EndpointCapabilities 从 sujiu-core 移到 sujiu-ai
3. 引入 ResolvedModelRoute(endpoint + model)
4. 把 previous_response_id 改成 Turn-local transient state
5. 把 PromptPlan 从 cache-centric 改为 semantic CompiledPrompt
6. CacheContinuity 降级成独立 diagnostics
```

### 第二阶段：Prompt 语义固定

```text
7. 给 PromptAssembler 补完整 Golden Tests
8. 固定 WorldBook 顺序和插入规则
9. 固定 PromptProfile 的有限位置
10. 清理不再需要的兼容抽象
```

### 第三阶段

再继续 UI 和新功能。

不要一边改这套底层抽象，一边继续加新的 AI 功能。

---

## 22. 最终判断标准

以后遇到任何新需求，只问四个问题：

```text
这是资源吗？
→ 放 Library / Conversation binding

这是 Prompt 语义吗？
→ 放 PromptAssembler

这是模型协议差异吗？
→ 放 ProtocolAdapter

这是 UI / OS 行为吗？
→ 放平台层
```

如果一个功能无法明确落入其中之一，先不要写代码。

---

## 23. 一句话架构

> Sujiu 的核心不是 Provider 抽象，也不是缓存抽象，而是“资源 → Prompt → 模型 → Tool Loop → Turn → Transcript”。

只要这条链保持简单、确定、可测试，角色卡、Persona、世界书、提示词、多轮对话、工具、Responses、Chat Completions、未来 Anthropic，以及后续真正需要时再增加的群聊能力，都可以自然挂在它的正确位置上。
