# LifeOS 接入阶段 1：通用扩展与双 Dock 基座实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 把当前单一 Business 扩展入口重构为可并存的通用 Turn Extension Registry 和 WorkspaceDockHost，同时保证 Business 行为、协议与视觉零变化。

**Architecture:** ACP 在每个可信 Turn 上下文中确定性选择至多一个扩展；扩展返回当前 Turn 的 prompt/MCP/session 策略，不再用进程级单扩展策略覆盖所有会话。Desktop 用通用 registry 管理 Business/Life Dock 的独立状态，本阶段只注册 Business，Life 使用测试桩且默认关闭。

**Tech Stack:** Rust / Tokio / ACP；React 19 / TypeScript / Tauri / Vitest-compatible Node tests。

---

### Task 1: 固化现有 Business 行为基线

**Files:**
- Modify: `crates/buzz-acp/src/product_extensions.rs`
- Test: `crates/buzz-acp/src/product_extensions.rs`
- Test: `desktop/src/extensions/AppExtensionProviders.test.mjs`
- Test: `desktop/src/extensions/AppExtensionDock.test.mjs`

**Step 1: 写失败测试**

为当前环境组合增加表格测试：无 Business 配置返回空 registry；配置完整时只返回 `business`；部分配置仍返回原有配置错误。Desktop 增加快照式结构测试，证明 provider 顺序、Business Dock 和顶部入口保持当前行为。

```rust
#[test]
fn business_extension_is_the_only_registered_extension_before_life_is_enabled() {
    let registry = load_from_test_config(valid_business_config(), disabled_life_config())
        .expect("valid config");
    assert_eq!(registry.ids(), ["business"]);
}
```

**Step 2: 运行测试并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p buzz-acp product_extensions
cd desktop && node --test src/extensions/AppExtensionProviders.test.mjs src/extensions/AppExtensionDock.test.mjs
```

Expected: FAIL，测试文件或 registry API 尚不存在。

**Step 3: 只添加测试所需的测试构造器，不改变运行逻辑**

把环境读取拆成可注入配置的纯函数，生产 `load_from_env` 继续走原路径。Desktop 测试通过源码/导出结构固定现状，不引入 Life UI。

**Step 4: 运行基线测试**

Expected: PASS；记录 Business 当前测试数量，供本阶段出口比较。

**Step 5: 提交**

```bash
git add crates/buzz-acp/src/product_extensions.rs desktop/src/extensions/AppExtensionProviders.test.mjs desktop/src/extensions/AppExtensionDock.test.mjs
git commit -s -m "test: pin extension platform behavior"
```

### Task 2: 引入可信 Turn 上下文和可组合扩展契约

**Files:**
- Modify: `crates/buzz-acp/src/turn_observer.rs`
- Modify: `crates/buzz-acp/src/pool.rs`
- Test: `crates/buzz-acp/src/turn_observer.rs`
- Test: `crates/buzz-acp/src/pool.rs`

**Step 1: 写失败测试**

测试 Host 构造的上下文包含已验证事件 ID/author、community、DM 或 channel、agent/turn/trace；测试扩展不能从 prompt 注入这些字段。

```rust
pub(crate) struct VerifiedTurnContext<'a> {
    pub source_event: Option<&'a Event>,
    pub source_event_id: Option<EventId>,
    pub source_pubkey: Option<PublicKey>,
    pub community_id: &'a str,
    pub conversation: VerifiedConversation,
    pub agent_id: &'a str,
    pub agent_turn_id: &'a str,
    pub trace_id: &'a str,
}
```

**Step 2: 运行定向测试并确认失败**

```bash
cargo test -p buzz-acp verified_turn_context -- --nocapture
```

Expected: FAIL，类型和 Host builder 尚不存在。

**Step 3: 实现最小契约**

将 `TurnExtensionRequest` 替换为 `VerifiedTurnContext`；新增：

```rust
pub(crate) enum TurnApplicability {
    NotApplicable,
    Applicable { priority: u16, reason: &'static str },
    Ambiguous { reason: &'static str },
}

pub(crate) struct TurnPolicy {
    pub mcp_mode: TurnMcpMode,
    pub base_prompt: Option<&'static str>,
    pub max_turn_duration: Option<Duration>,
    pub disable_memory: bool,
    pub requires_fresh_session: bool,
}

pub(crate) trait TurnExtension: Send + Sync {
    fn id(&self) -> &'static str;
    fn classify_turn(&self, context: &VerifiedTurnContext<'_>) -> TurnApplicability;
    fn begin_turn<'a>(&'a self, context: VerifiedTurnContext<'a>)
        -> TurnExtensionFuture<'a, Result<Option<Box<dyn TurnExtensionAccess>>, String>>;
}
```

`TurnExtensionAccess` 增加 `policy(&self) -> &TurnPolicy`。不要把 Life capability、工具名或 workspace 选择写进该模块。

**Step 4: 修改 session/prompt 调用点**

在 `pool.rs` 中先构造可信上下文、再选择扩展、再计算本 Turn 的有效 MCP/prompt/memory/timeout。`create_session_and_apply_model_with_turn_mcp` 接收 `TurnRuntimePolicy`，而不是从 `PromptContext` 隐式读取扩展覆盖值。无扩展时必须与原行为字节级等价。

**Step 5: 运行测试**

```bash
cargo test -p buzz-acp verified_turn_context
cargo test -p buzz-acp turn_runtime_policy
```

Expected: PASS。

**Step 6: 提交**

```bash
git add crates/buzz-acp/src/turn_observer.rs crates/buzz-acp/src/pool.rs
git commit -s -m "refactor: make turn extension policy per turn"
```

### Task 3: 实现确定性 Extension Registry

**Files:**
- Create: `crates/buzz-acp/src/turn_extension_registry.rs`
- Modify: `crates/buzz-acp/src/lib.rs`
- Modify: `crates/buzz-acp/src/product_extensions.rs`
- Modify: `crates/buzz-acp/src/business_agent.rs`
- Test: `crates/buzz-acp/src/turn_extension_registry.rs`
- Test: `crates/buzz-acp/src/business_agent.rs`

**Step 1: 写选择规则测试**

覆盖零匹配、单匹配、优先级胜出、同优先级歧义、显式资源域优先、classifier 错误 fail-closed。

```rust
assert_eq!(registry.select(&context)?.map(|x| x.id()), Some("business"));
assert!(matches!(registry.select(&ambiguous), Err(RegistryError::Ambiguous { .. })));
```

**Step 2: 运行并确认失败**

```bash
cargo test -p buzz-acp turn_extension_registry
```

Expected: FAIL，registry 不存在。

**Step 3: 实现 registry**

`TurnExtensionRegistry` 保存按 ID 排序的扩展；选择只依赖可信上下文和扩展的确定性分类。任何相同最高优先级歧义都返回需要澄清的稳定错误，不同时签发两个安全域委托。

**Step 4: 迁移 Business Extension**

Business classifier 只识别已有 Business 显式资源/配置语义；其 prompt、MCP replace、fresh session、memory 和时限保持原值。`product_extensions::load_from_env` 返回 `Arc<TurnExtensionRegistry>`，同时可注册默认关闭的 Life factory，但本计划不实现 Life 策略。

**Step 5: 运行回归**

```bash
cargo test -p buzz-acp turn_extension_registry business_agent
```

Expected: PASS；Business approval 精确命令测试仍全部通过。

**Step 6: 提交**

```bash
git add crates/buzz-acp/src/turn_extension_registry.rs crates/buzz-acp/src/lib.rs crates/buzz-acp/src/product_extensions.rs crates/buzz-acp/src/business_agent.rs
git commit -s -m "feat: add deterministic turn extension registry"
```

### Task 4: 建立通用 Workspace Dock 类型、registry 和独立状态

**Files:**
- Create: `desktop/src/features/workspace-dock/WorkspaceDockHost.tsx`
- Create: `desktop/src/features/workspace-dock/WorkspaceDockRegistry.ts`
- Create: `desktop/src/features/workspace-dock/workspaceDockTypes.ts`
- Create: `desktop/src/features/workspace-dock/workspaceDockStore.ts`
- Create: `desktop/src/features/workspace-dock/workspaceDockStore.test.mjs`
- Create: `desktop/src/features/workspace-dock/WorkspaceDockHost.test.mjs`
- Modify: `desktop/src/extensions/AppExtensionLayout.tsx`
- Modify: `desktop/src/extensions/AppExtensionDock.tsx`

**Step 1: 写失败测试**

测试两个 extension state 完全隔离、一次只显示一个、隐藏 dock 保持 mounted、切换时 Dirty 拒绝、非法 extension ID 拒绝。

```ts
export type WorkspaceDockState = {
  open: boolean;
  active: boolean;
  pinned: boolean;
  followConversation: boolean;
  fullscreen: boolean;
  currentResource: WorkspaceResource | null;
  history: WorkspaceResource[];
  dirty: boolean;
};
```

**Step 2: 运行并确认失败**

```bash
cd desktop
node --test src/features/workspace-dock/*.test.mjs
```

Expected: FAIL，模块不存在。

**Step 3: 实现纯状态层**

每个 extension ID 使用独立 store slice。Host 只允许一个 `activeExtensionId`；隐藏 Dock 用 `width: 0` 和 `visibility: hidden`，不卸载 iframe。Dirty guard 返回结构化 decision，由 UI 决定显示确认框。

**Step 4: 实现 registry 类型**

```ts
export type WorkspaceDockExtension = {
  id: "business" | "life";
  title: string;
  scheme: "biz" | "life";
  origin: string;
  homeUrl: string;
  resolveResource(input: string | object): WorkspaceResource | null;
  Provider: React.ComponentType<React.PropsWithChildren>;
  Dock: React.ComponentType;
  TopChromeAction: React.ComponentType;
};
```

Registry 启动时拒绝重复 ID/scheme、无效 origin 和 homeUrl 跨 origin。

**Step 5: 接入 AppExtensionLayout**

`AppExtensionLayout` 渲染 `WorkspaceDockHost`；本 Task 仍只注册 Business adapter，视觉结构不变。

**Step 6: 运行测试**

Expected: PASS。

**Step 7: 提交**

```bash
git add desktop/src/features/workspace-dock desktop/src/extensions/AppExtensionLayout.tsx desktop/src/extensions/AppExtensionDock.tsx
git commit -s -m "feat: add generic workspace dock host"
```

### Task 5: 把 Business Dock 适配到通用 Host

**Files:**
- Create: `desktop/src/features/business-dock/businessDockExtension.tsx`
- Modify: `desktop/src/extensions/AppExtensionProviders.tsx`
- Modify: `desktop/src/extensions/AppExtensionTopChromeActions.tsx`
- Modify: `desktop/src/features/business-dock/BusinessDockProvider.tsx`
- Modify: `desktop/src/features/business-dock/BusinessDockTopChromeAction.tsx`
- Test: `desktop/src/features/business-dock/BusinessDockProvider.test.mjs`
- Test: `desktop/src/features/business-dock/businessDockBridge.test.mjs`
- Test: `desktop/src/features/business-dock/businessResourceResolver.test.mjs`

**Step 1: 写 adapter contract 测试**

断言 Business adapter 的 ID/scheme/origin/home、resolver、Provider/Dock/Action 指向原实现；既有 Business bridge wire schema 不变。

**Step 2: 运行并确认失败**

```bash
node --test src/features/business-dock/*.test.mjs
```

Expected: FAIL，新 adapter 尚不存在。

**Step 3: 实现薄 adapter**

不得重写 Business Bridge 或 Embed Session。Adapter 只把现有导出注册到通用接口，并把通用 active/open 操作映射到现有 Business provider。

**Step 4: 运行全部 Desktop 单元测试和 E2E 构建**

```bash
pnpm test
pnpm build:e2e
```

Expected: PASS，Business UI 无新增文案或布局偏移。

**Step 5: 提交**

```bash
git add desktop/src/extensions desktop/src/features/business-dock desktop/src/features/workspace-dock
git commit -s -m "refactor: register business dock through generic host"
```

### Task 6: 增加默认关闭的 Life 扩展占位和配置校验

**Files:**
- Create: `crates/buzz-acp/src/life_agent.rs`
- Create: `crates/buzz-acp/src/life_agent_prompt.md`
- Modify: `crates/buzz-acp/src/product_extensions.rs`
- Modify: `crates/buzz-acp/src/config.rs`
- Test: `crates/buzz-acp/src/life_agent.rs`
- Create: `desktop/src/features/life-dock/lifeDockConfig.ts`
- Create: `desktop/src/features/life-dock/lifeDockConfig.test.mjs`

**Step 1: 写开关和配置测试**

覆盖默认关闭、父子开关非法组合、Life gateway/API/MCP command 缺失、精确 HTTP(S) origin、禁止路径/userinfo/query/fragment。

**Step 2: 运行并确认失败**

```bash
cargo test -p buzz-acp life_agent
cd desktop && node --test src/features/life-dock/lifeDockConfig.test.mjs
```

Expected: FAIL。

**Step 3: 实现只会拒绝的占位扩展**

关闭时不注册；开启但依赖阶段未完成时，启动配置验证必须给出明确错误。不得提供假 MCP、样例数据或数据库直连降级。

**Step 4: 验证默认配置**

无任何 Life 环境变量时，Pacioli 必须正常启动并只加载现有扩展。

**Step 5: 提交**

```bash
git add crates/buzz-acp/src/life_agent.rs crates/buzz-acp/src/life_agent_prompt.md crates/buzz-acp/src/product_extensions.rs crates/buzz-acp/src/config.rs desktop/src/features/life-dock/lifeDockConfig.ts desktop/src/features/life-dock/lifeDockConfig.test.mjs
git commit -s -m "feat: add disabled life extension configuration"
```

### Task 7: 阶段出口验证

**Files:**
- Modify only if a regression test needs correction; no new feature scope.

**Step 1: Rust 验证**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo fmt --all -- --check
cargo clippy -p buzz-acp --all-targets -- -D warnings
cargo test -p buzz-acp
```

Expected: 全部 PASS。

**Step 2: Desktop 验证**

```bash
cd /Users/aaronli/Projects/Paqiaoli/desktop
pnpm check:px-text
pnpm test
pnpm build:e2e
pnpm test:e2e:smoke
```

Expected: 全部 PASS。

**Step 3: 开关关闭回归**

启动不设置任何 `LIFE_*` 环境变量的 ACP/Desktop，执行一个普通会话和现有 Business 会话；确认普通 MCP、Business MCP、Business Dock 均与基线一致。

**Step 4: 请求代码审查**

使用 `superpowers:requesting-code-review`，重点检查：Core 是否出现 Life 产品策略、是否可能同 Turn 选择两个安全域、Business wire schema 是否变化、隐藏 Dock 是否被卸载。
