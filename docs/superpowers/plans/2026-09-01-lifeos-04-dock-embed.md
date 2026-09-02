# LifeOS 接入阶段 4：Life Dock、Embed Session、Bridge 与 life:// 实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Work in both repositories and preserve unrelated LifeOS changes.

**Goal:** 在 Pacioli Desktop 中加入与 Business Dock 并行、状态独立的 Life Dock，通过单次 Embed Session 安全显示 LifeOS，并支持受控 Bridge、`life://` 资源定位和会话结果联动。

**Architecture:** Pacioli 使用阶段 1 的 `WorkspaceDockHost` 注册 Life adapter；桌面从 Gateway 获取一次性 embed code，LifeOS `/embed/bootstrap` 原子兑换成独立 HttpOnly Dock Session。Host/iframe 只通过精确 origin/source/nonce/schema 的 Bridge 交换导航和最小状态，不传授权或个人正文。

**Tech Stack:** React 19 / TypeScript / Tauri 2 / postMessage；Next.js 15 / Prisma session；Playwright。

---

### Task 1: 实现并穷举测试 `life://` resolver

**Files (Pacioli):**
- Create: `desktop/src/features/life-dock/lifeResourceResolver.ts`
- Create: `desktop/src/features/life-dock/lifeResourceResolver.test.mjs`
- Modify: `desktop/src/features/workspace-dock/workspaceDockTypes.ts`

**Step 1: 写失败测试**

覆盖全部固定映射，以及空 ID、超过 128 字符、路径穿越、额外层级、双重编码、userinfo、fragment、未知/重复 query、token/workspace/email 等敏感 query 拒绝。

```ts
assert.deepEqual(resolveLifeResource("life://action/a-1"), {
  version: 1,
  extensionId: "life",
  type: "action",
  id: "a-1",
  path: "/embed/actions/a-1",
});
assert.equal(resolveLifeResource("life://action/%252e%252e"), null);
```

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli/desktop
node --test src/features/life-dock/lifeResourceResolver.test.mjs
```

Expected: FAIL，resolver 不存在。

**Step 3: 实现固定映射**

支持 dashboard/domain/goal/project/action/calendar/journal/knowledge/review/ai-execution/draft。ID 只 decode 一次并逐字符检查；calendar 只接受 `yyyy-mm-dd`；输出 path 总长度设上限。链接不解析权限或执行命令。

**Step 4: 运行并提交**

```bash
node --test src/features/life-dock/lifeResourceResolver.test.mjs
cd ..
. ./bin/activate-hermit
git add desktop/src/features/life-dock/lifeResourceResolver.ts desktop/src/features/life-dock/lifeResourceResolver.test.mjs desktop/src/features/workspace-dock/workspaceDockTypes.ts
git commit -s -m "feat: resolve safe life resource links"
```

### Task 2: 实现 Life Bridge 信封与严格入站验证

**Files (Pacioli):**
- Create: `desktop/src/features/life-dock/lifeDockBridge.ts`
- Create: `desktop/src/features/life-dock/lifeDockBridge.test.mjs`

**Step 1: 写失败测试**

测试 V2 导航/资源/action/dirty，V3 auth；错误 origin、错误 `event.source`、错误 nonce、未知 version/type、额外字段、超长文本/数组、正文/token/cookie/workspace 字段都拒绝。

**Step 2: 运行并确认失败**

```bash
node --test src/features/life-dock/lifeDockBridge.test.mjs
```

Expected: FAIL。

**Step 3: 实现纯 parser 和 sender**

Host → Life 仅：`HOST_INIT`、`SET_THEME`、`REFRESH`、`NAVIGATE`、`REQUEST_CURRENT_RESOURCE`、`CHECK_AUTH`、`LOGOUT`。

Life → Host 仅：`LIFE_READY`、`TITLE_CHANGED`、`ROUTE_CHANGED`、`RESOURCE_CHANGED`、`ACTION_COMPLETED`、`ACTION_FAILED`、`DATA_CHANGED`、`DIRTY_STATE_CHANGED`、`AUTH_STATUS`、`AUTH_REQUIRED`、`SESSION_EXPIRED`。

不要修改 `businessDockBridge.ts` 的 wire schema。可以提取无产品语义的内部校验 helper，但先用 Business 全测试证明等价。

**Step 4: 运行并提交**

```bash
node --test src/features/life-dock/lifeDockBridge.test.mjs src/features/business-dock/businessDockBridge.test.mjs
cd .. && . ./bin/activate-hermit
git add desktop/src/features/life-dock/lifeDockBridge.ts desktop/src/features/life-dock/lifeDockBridge.test.mjs
git commit -s -m "feat: add validated life dock bridge"
```

### Task 3: 在 Gateway 完成可供 Dock 使用的 Embed Session API

**Files (Pacioli):**
- Modify: `services/life-auth-gateway/src/embed.rs`
- Modify: `services/life-auth-gateway/src/http.rs`
- Modify: `services/life-auth-gateway/src/store.rs`
- Modify: `services/life-auth-gateway/tests/embed_session.rs`
- Create: `desktop/src/features/life-dock/lifeEmbedSession.ts`
- Create: `desktop/src/features/life-dock/lifeEmbedSession.test.mjs`

**Step 1: 写失败测试**

覆盖 OIDC access token → Life user 映射、target resource/path allowlist、32-byte code/hash-only、单次并发消费、IP/UA risk facts、logout/解绑/session revoke、deep-link callback 严格格式。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli
. ./bin/activate-hermit
cargo test -p life-auth-gateway --test embed_session
cd desktop && node --test src/features/life-dock/lifeEmbedSession.test.mjs
```

Expected: FAIL 或缺少 desktop helper。

**Step 3: 完成 Gateway 响应**

`POST /v1/embed-sessions` 返回单次 `embedUrl` 或 desktop callback code；code 不进入日志。消费返回给 LifeOS 的 session assertion 绑定 user、workbench session、deployment、target 和 trace，并使用与 call grant 不同的 audience。

**Step 4: 实现 Desktop helper**

回调固定为 `pacioli://auth/life-bootstrap?code=<43-character-base64url>`；code 正则为 43 位 base64url。最多自动恢复一次。任何跨 origin target 返回 null。

**Step 5: 运行并提交**

```bash
cargo test -p life-auth-gateway --test embed_session
cd desktop && node --test src/features/life-dock/lifeEmbedSession.test.mjs
cd .. && . ./bin/activate-hermit
git add services/life-auth-gateway desktop/src/features/life-dock/lifeEmbedSession.ts desktop/src/features/life-dock/lifeEmbedSession.test.mjs
git commit -s -m "feat: issue life dock embed sessions"
```

### Task 4: 在 LifeOS 建立独立 Dock Session 和 bootstrap

**Files (LifeOS):**
- Modify: `prisma/schema.prisma`
- Create: `lib/embed/session.ts`
- Create: `lib/embed/csrf.ts`
- Create: `lib/embed/gateway-client.ts`
- Create: `app/embed/bootstrap/route.ts`
- Create: `app/embed/logout/route.ts`
- Create: `app/api/embed/session/route.ts`
- Create: `scripts/test-embed-session.mjs`
- Modify: `middleware.ts`

**Step 1: 写失败测试**

测试 code 只消费一次、建立独立 `HttpOnly; Secure; SameSite=None` Dock cookie、CSRF、target allowlist、普通 browser session 不等同 Dock session、解绑/登出/过期失效、错误后不产生 cookie。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/life-os
node scripts/test-embed-session.mjs
```

Expected: FAIL。

**Step 3: 增加 Dock Session model**

保存 session token hash、user/workbench session/deployment、expires/revoked/lastSeen/trace；cookie 中只放随机 token。API heartbeat 重新检查 user 和 workspace membership。

**Step 4: 实现 bootstrap**

服务端向 Gateway 原子兑换 code，建立 Dock Session，重定向到 assertion 内的 allowlist target。客户端不能通过 query 覆盖 target/user/workspace。

**Step 5: 运行并提交**

```bash
npm run prisma:generate
node scripts/test-embed-session.mjs
git add prisma/schema.prisma lib/embed app/embed/bootstrap/route.ts app/embed/logout/route.ts app/api/embed/session/route.ts scripts/test-embed-session.mjs middleware.ts
git commit -m "feat: add isolated life dock sessions"
```

### Task 5: 建立 LifeOS embed layout 和固定资源路由

**Files (LifeOS):**
- Create: `app/embed/layout.tsx`
- Create: `components/embed/life-bridge-client.tsx`
- Create: `components/embed/embed-shell.tsx`
- Create: `app/embed/dashboard/page.tsx`
- Create: `app/embed/domains/[id]/page.tsx`
- Create: `app/embed/goals/[id]/page.tsx`
- Create: `app/embed/projects/[id]/page.tsx`
- Create: `app/embed/actions/[id]/page.tsx`
- Create: `app/embed/calendar/page.tsx`
- Create: `app/embed/journal/[id]/page.tsx`
- Create: `app/embed/knowledge/[id]/page.tsx`
- Create: `app/embed/reviews/[id]/page.tsx`
- Create: `app/embed/ai-executions/[id]/page.tsx`
- Create: `app/embed/drafts/[id]/page.tsx`
- Create: `scripts/test-embed-routes-static.mjs`
- Create: `scripts/test-life-bridge.mjs`

**Step 1: 写失败测试**

固定 route 清单、无主导航/外壳、Dock session required、Bridge nonce handshake、theme/refresh/navigation/current-resource/auth/logout、Dirty State、消息 bounds。

**Step 2: 运行并确认失败**

```bash
node scripts/test-embed-routes-static.mjs
node scripts/test-life-bridge.mjs
```

Expected: FAIL。

**Step 3: 提取可复用页面内容**

若普通 page 当前无法复用，先把显示主体抽到 `components/pages/*Content.tsx`，普通 route 与 embed route 同时引用。不要从一个 Next page module 直接 import 另一个 page module。Embed shell 不显示普通 AppShell 导航，但领域按钮继续按 Dock Session 权限工作。

**Step 4: 实现 iframe Bridge client**

只向配置的 Pacioli parent origin 发送；首次 `HOST_INIT` 固定 sessionNonce；不把个人正文、token、cookie、workspace、权限放进 payload。`DATA_CHANGED` 只发 resource type/id/version/trace。

**Step 5: 运行并提交**

```bash
node scripts/test-embed-routes-static.mjs
node scripts/test-life-bridge.mjs
npm run build
git add app/embed components/embed components/pages scripts/test-embed-routes-static.mjs scripts/test-life-bridge.mjs
git commit -m "feat: add lifeos embedded resource routes"
```

### Task 6: 配置精确 CSP 和 frame-ancestors

**Files (Pacioli):**
- Modify: `desktop/src-tauri/tauri.conf.json`
- Create: `desktop/src/features/life-dock/lifeDockCsp.test.mjs`

**Files (LifeOS):**
- Modify: `next.config.ts`
- Create: `lib/embed/csp.ts`
- Create: `scripts/test-embed-csp-static.mjs`

**Step 1: 写失败测试**

Pacioli `frame-src` 只含已验证 Business/Life origin；禁止 `*`、`http:`、`https:` 通配。LifeOS embed response 的 `frame-ancestors` 只含配置的 Pacioli origin，普通 route 不被无意开放。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli/desktop
node --test src/features/life-dock/lifeDockCsp.test.mjs
cd /Users/aaronli/Projects/life-os
node scripts/test-embed-csp-static.mjs
```

Expected: FAIL。

**Step 3: 实现环境驱动的构建期 CSP**

origin validator 与 Dock config 同一规则。生产缺失 origin 时禁止启用 Life Dock，不自动放宽。

**Step 4: 运行并分别提交**

在两个仓库分别提交各自 CSP 文件，避免跨仓库混合提交。

### Task 7: 实现 Life Dock Provider、Browser、Toolbar 和入口

**Files (Pacioli):**
- Create: `desktop/src/features/life-dock/LifeDockProvider.tsx`
- Create: `desktop/src/features/life-dock/LifeDock.tsx`
- Create: `desktop/src/features/life-dock/LifeDockBrowser.tsx`
- Create: `desktop/src/features/life-dock/LifeDockToolbar.tsx`
- Create: `desktop/src/features/life-dock/LifeDockTopChromeAction.tsx`
- Create: `desktop/src/features/life-dock/lifeDockExtension.tsx`
- Create: `desktop/src/features/life-dock/lifeDockNavigation.ts`
- Create: `desktop/src/features/life-dock/lifeDockPreferences.ts`
- Create: `desktop/src/features/life-dock/index.ts`
- Create: `desktop/src/features/life-dock/LifeDockProvider.test.mjs`
- Create: `desktop/src/features/life-dock/lifeDockNavigation.test.mjs`
- Modify: `desktop/src/extensions/AppExtensionProviders.tsx`
- Modify: `desktop/src/extensions/AppExtensionTopChromeActions.tsx`

**Step 1: 写失败测试**

覆盖 Life/Business state 独立、同时仅一个可见、iframe 保持挂载、back/forward/home/refresh、pin/follow/fullscreen、dirty guard、session expired 一次恢复、开关关闭不注册。

**Step 2: 运行并确认失败**

```bash
cd /Users/aaronli/Projects/Paqiaoli/desktop
node --test src/features/life-dock/*.test.mjs src/features/workspace-dock/*.test.mjs
```

Expected: FAIL。

**Step 3: 实现组件**

沿用现有 Business Dock 的交互密度和可访问性，但不共享 iframeRef/sessionNonce/history/dirty/preferences。所有可读文本用既有 rem Tailwind token，不新增 px/arbitrary text size。

**Step 4: 注册 adapter**

只有 `LIFE_DOCK_ENABLED` 且 config 有效时注册。Business 和 Life 顶部入口由 WorkspaceDockHost 排列；不要让当前频道或当前打开 Dock 成为 Agent 授权依据。

**Step 5: 运行并提交**

```bash
pnpm check:px-text
pnpm test
pnpm build:e2e
cd .. && . ./bin/activate-hermit
git add desktop/src/features/life-dock desktop/src/extensions desktop/src/features/workspace-dock
git commit -s -m "feat: add independent life workspace dock"
```

### Task 8: 接入消息中的 `life://` 与可信 Turn resourceRefs

**Files (Pacioli):**
- Create: `desktop/src/features/life-dock/lifeLinkHandler.ts`
- Create: `desktop/src/features/life-dock/lifeLinkHandler.test.mjs`
- Modify: `desktop/src/features/messages/MessageContent.tsx`
- Modify: `desktop/src/features/workspace-dock/WorkspaceDockHost.tsx`
- Test: relevant message renderer test under `desktop/src/features/messages/`

**Step 1: 写失败测试**

普通 click 打开 Dock；Cmd/Ctrl click 系统浏览器；普通新消息不自动打开；只有当前受信 Turn 的验证 `resourceRefs` 且 followConversation 开启才请求导航；pinned/dirty/不同安全域只提示。

**Step 2: 运行并确认失败**

运行新 handler test 和消息 renderer 定向测试，Expected: FAIL。

**Step 3: 实现 handler**

消息里的字符串仍走严格 resolver。自动导航的数据来源必须是 ACP 受信扩展结果对象，不从 Markdown 文本反推 resource ref。

**Step 4: 运行并提交**

运行 Desktop 单测和 `pnpm build:e2e` 后提交明确文件。

### Task 9: 编写真实 Dock E2E 和截图验证

**Files (Pacioli):**
- Create: `desktop/tests/e2e/lifeDock.spec.ts`
- Modify: `desktop/playwright.config.ts`
- Modify: `desktop/tests/helpers/e2eBridge.ts` only if the test bridge needs Life fixtures

**Files (LifeOS):**
- Create: `scripts/test-embed-browser.mjs`

**Step 1: 写 E2E**

覆盖登录 bootstrap、resource navigation、theme、history、dirty 切换阻止、pin、Business/Life 切换保持 iframe、session expiry/recovery、恶意 postMessage 被忽略。

**Step 2: 构建正确模式并运行**

```bash
cd /Users/aaronli/Projects/Paqiaoli/desktop
pnpm build:e2e
pnpm test:e2e:smoke
```

所有 screenshot 前调用 `waitForAnimations(page)`；截图按主题 locator 裁切，并用 `shasum -a 256` 确认不同状态不是重复像素。

**Step 3: LifeOS 浏览器测试**

```bash
cd /Users/aaronli/Projects/life-os
node scripts/test-embed-browser.mjs
npm run build
```

Expected: PASS。

**Step 4: 分别提交并请求审查**

使用 `superpowers:requesting-code-review`，重点检查 CSP、cookie/CSRF、postMessage origin+source+nonce、自动导航数据来源、Business Dock 回归。
