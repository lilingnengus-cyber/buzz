#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import http from "node:http";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const expectedTools = [
  "prepare_crm_creation",
  "approve_crm_creation",
  "prepare_crm_update",
  "approve_crm_update",
  "prepare_crm_followup",
  "approve_crm_followup",

  "search_crm_opportunities",
  "get_crm_opportunity",
  "get_inventory_count_approval_preview",
  "search_inventory_counts",
  "get_inventory_count",
  "search_inventory_count_options",
  "prepare_inventory_count_creation",
  "approve_inventory_count_creation",
  "prepare_inventory_count_submission",
  "approve_inventory_count_submission",
  "prepare_inventory_count_posting",
  "approve_inventory_count_posting",
  "prepare_inventory_count_cancellation",
  "approve_inventory_count_cancellation",

  "prepare_sales_return_reversal",
  "approve_sales_return_reversal",
  "prepare_purchase_return_reversal",
  "approve_purchase_return_reversal",
  "prepare_sales_return_cancellation",
  "approve_sales_return_cancellation",
  "prepare_purchase_return_cancellation",
  "approve_purchase_return_cancellation",

  "update_sales_return_draft",
  "update_purchase_return_draft",
  "search_sales_returns",
  "search_purchase_returns",
  "get_sales_return_source",
  "get_purchase_return_source",
  "get_sales_return_approval_preview",
  "get_purchase_return_approval_preview",
  "create_sales_return_draft",
  "create_purchase_return_draft",
  "approve_sales_return",
  "approve_purchase_return",
  "prepare_sales_return_inspection",
  "approve_sales_return_inspection",
  "prepare_purchase_return_dispatch",
  "approve_purchase_return_dispatch",
  "prepare_purchase_return_acknowledgment",
  "approve_purchase_return_acknowledgment",

  "search_shipments",
  "search_goods_receipts",
  "search_inventory_openings",
  "prepare_shipment_reversal",
  "approve_shipment_reversal",
  "prepare_goods_receipt_reversal",
  "approve_goods_receipt_reversal",
  "prepare_inventory_opening_reversal",
  "approve_inventory_opening_reversal",

  "prepare_sales_order_cancellation",
  "prepare_purchase_order_cancellation",
  "approve_sales_order_cancellation",
  "approve_purchase_order_cancellation",

  "get_customer_receipt_allocations",
  "get_supplier_payment_allocations",
  "prepare_customer_receipt_reversal",
  "prepare_supplier_payment_reversal",
  "prepare_receivable_allocation_reversal",
  "prepare_payable_allocation_reversal",
  "approve_customer_receipt_reversal",
  "approve_supplier_payment_reversal",
  "approve_receivable_allocation_reversal",
  "approve_payable_allocation_reversal",

  "analyze_cross_domain_risks",
  "analyze_inventory_risks",
  "analyze_order_profit_risks",
  "analyze_purchase_cost_risks",
  "analyze_receivable_risks",
  "approve_goods_receipt",
  "approve_inventory_opening",
  "approve_shipment",
  "approve_purchase_order",
  "approve_sales_order",
  "prepare_receivable_allocation",
  "approve_receivable_allocation",
  "prepare_payable_allocation",
  "approve_payable_allocation",

  "approve_customer_receipt",
  "get_customer_receipt_approval_preview",
  "approve_supplier_payment",
  "get_supplier_payment_approval_preview",

  "create_customer_receipt_draft",
  "create_goods_receipt_draft",
  "create_inventory_opening_draft",
  "create_purchase_order_draft",
  "create_sales_order_draft",
  "create_shipment_draft",
  "create_supplier_payment_draft",
  "explain_profit_change",
  "get_action_proposal",
  "get_action_recommendations",
  "get_approval_draft",
  "get_business_anomaly",
  "get_business_data_quality",
  "get_finding_lifecycle",
  "get_goods_receipt_approval_preview",
  "get_inventory_opening_approval_preview",
  "get_shipment_approval_preview",
  "get_management_profit_report",
  "get_management_report_snapshot",
  "get_operating_dashboard",
  "get_profit_evidence",
  "get_purchase_order",
  "get_purchase_order_approval_preview",
  "get_sales_order",
  "get_sales_order_approval_preview",
  "get_work_item",
  "query_inventory_balance",
  "query_order_profit",
  "query_payables",
  "query_profitability_by_dimension",
  "query_receivables",
  "search_business_anomalies",
  "search_business_master_data",
  "search_customer_receipts",
  "search_supplier_payments",
  "search_receivables",
  "search_payables",

  "search_purchase_orders",
  "search_sales_orders",
  "search_work_items",
  "update_sales_order_draft",
  "update_purchase_order_draft",
].map((name) => `business-read-mcp__${name}`);

function jsonResponse(response, body) {
  response.writeHead(200, { "content-type": "application/json" });
  response.end(JSON.stringify(body));
}

const capacityFixture = process.env.BUSINESS_MCP_CAPACITY_FIXTURE
  ? JSON.parse(await readFile(process.env.BUSINESS_MCP_CAPACITY_FIXTURE, "utf8")) : null;
const probeId = "00000000-0000-4000-8000-000000000001";
const traceId = capacityFixture?.traceId ?? probeId;
const capacityInput = process.env.BUSINESS_MCP_CAPACITY_INPUT ? JSON.parse(await readFile(process.env.BUSINESS_MCP_CAPACITY_INPUT, "utf8")) : capacityFixture ? {
  inventoryCountId: capacityFixture.document.source.id,
  command: capacityFixture.document.operation.command,
} : null;
const capacityPage = process.env.BUSINESS_MCP_CAPACITY_PAGE ? JSON.parse(await readFile(process.env.BUSINESS_MCP_CAPACITY_PAGE, "utf8")) : null;
if (capacityPage) {
  capacityPage.traceId = traceId;
  capacityPage.items[0].traceId = traceId;
}
const capacityPageInput = capacityPage ? {documentId:capacityFixture.item.id,documentType:capacityFixture.documentType,previewHash:capacityFixture.previewHash,offset:capacityPage.items[0].previewPagination.offset,limit:20} : null;
const capacityCalls = [];
const capacityServer = http.createServer((request, response) => {
  let raw = "";
  request.setEncoding("utf8");
  request.on("data", chunk => { raw += chunk; });
  request.on("end", () => {
    const input = raw ? JSON.parse(raw) : {};
    capacityCalls.push({ path: request.url, input });
    if (request.url === "/internal/agent-delegations/consume") {
      jsonResponse(response, {
        delegationId: probeId, enterpriseUserId: probeId, identityBindingId: probeId,
        sourceBuzzEventId: "a".repeat(64), sourceBuzzPubkey: "b".repeat(64), sourceChannelId: "capacity-probe",
        agentId: "acceptance-agent", agentTurnId: "acceptance-turn", traceId,
        usedCalls: 1, maxCalls: 20, requiredScope: input.requiredScope,
        effectiveGrant: { capability: input.requiredScope, dataScope: { mode: "unrestricted" }, obligations: [] },
      });
    } else if (request.url === "/v1/write/prepare_inventory_count_submission") {
      jsonResponse(response, capacityFixture);
    } else if (request.url === "/v1/read/get_inventory_count_approval_preview" && capacityPage) {
      jsonResponse(response, capacityPage);
    } else if (request.url === "/internal/agent-audit") {
      response.writeHead(204); response.end();
    } else { response.writeHead(404); response.end(); }
  });
});
await new Promise((resolve, reject) => { capacityServer.once("error", reject); capacityServer.listen(0, "127.0.0.1", resolve); });
const capacityAddress = capacityServer.address();
const capacityUrl = `http://127.0.0.1:${capacityAddress.port}/`;
const payloadLimit = process.env.BUSINESS_CAPACITY_PAYLOAD_BYTES ?? "131072";
const textLimit = process.env.BUSINESS_CAPACITY_TEXT_BYTES ?? "51200";
const contextLimit = process.env.BUSINESS_CAPACITY_CONTEXT_TOKENS ?? "200000";
const observedRequests = [];
const modelServer = http.createServer((request, response) => {
  let raw = "";
  request.setEncoding("utf8");
  request.on("data", (chunk) => {
    raw += chunk;
  });
  request.on("end", () => {
    const body = JSON.parse(raw);
    observedRequests.push(body);
    jsonResponse(response, {
      id: "business-agent-runtime-acceptance",
      object: "chat.completion",
      created: 0,
      model: "business-agent-probe",
      choices: [
        {
          index: 0,
          message: capacityFixture && observedRequests.length === 1
            ? { role: "assistant", content: null, tool_calls: [{ id: "capacity-call", type: "function", function: { name: "business-read-mcp__prepare_inventory_count_submission", arguments: JSON.stringify(capacityInput) } }] }
            : capacityPage && observedRequests.length === 2
              ? {role:"assistant",content:null,tool_calls:[{id:"capacity-page-call",type:"function",function:{name:"business-read-mcp__get_inventory_count_approval_preview",arguments:JSON.stringify(capacityPageInput)}}]}
              : { role: "assistant", content: "probe complete" },
          finish_reason: capacityFixture && (observedRequests.length === 1 || (capacityPage && observedRequests.length === 2)) ? "tool_calls" : "stop",
        },
      ],
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    });
  });
});

await new Promise((resolve, reject) => {
  modelServer.once("error", reject);
  modelServer.listen(0, "127.0.0.1", resolve);
});

const address = modelServer.address();
assert(address && typeof address !== "string");
const agent = spawn(process.env.BUSINESS_AGENT_TEST_BINARY ?? path.join(repoRoot, "target/debug/buzz-agent"), [], {
  cwd: repoRoot,
  env: {
    ...process.env,
    BUZZ_AGENT_PROVIDER: "openai",
    OPENAI_COMPAT_API_KEY: "acceptance-probe-not-used",
    OPENAI_COMPAT_MODEL: "business-agent-probe",
    OPENAI_COMPAT_API: "chat",
    OPENAI_COMPAT_BASE_URL: `http://127.0.0.1:${address.port}/v1`,
    BUZZ_AGENT_NO_HINTS: "1",
    ...(capacityFixture ? { BUZZ_AGENT_MAX_TOOL_RESULT_TEXT_BYTES: textLimit, BUZZ_AGENT_MAX_CONTEXT_TOKENS: contextLimit } : {}),
  },
  stdio: ["pipe", "pipe", "inherit"],
});

let nextRequestId = 0;
const pending = new Map();
const lines = readline.createInterface({ input: agent.stdout });
lines.on("line", (line) => {
  const message = JSON.parse(line);
  if (message.id === undefined || message.id === null) return;
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
  else waiter.resolve(message.result);
});

function request(method, params) {
  const id = nextRequestId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    agent.stdin.write(
      `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
    );
  });
}

const timeout = setTimeout(() => {
  agent.kill("SIGKILL");
}, 60_000);

try {
  const initialized = await request("initialize", {
    protocolVersion: 2,
    clientCapabilities: {},
    clientInfo: { name: "business-agent-runtime-acceptance", version: "1" },
  });
  assert.equal(
    initialized.agentInfo?.name ?? initialized.serverInfo?.name,
    "buzz-agent",
  );

  const session = await request("session/new", {
    cwd: repoRoot,
    systemPrompt: "Business Agent runtime acceptance probe.",
    mcpServers: [
      {
        name: "business-read-mcp",
        command: process.env.BUSINESS_MCP_TEST_BINARY ?? path.join(repoRoot, "target/debug/business-read-mcp"),
        args: [],
        env: [
          { name: "BUSINESS_READ_ADAPTER", value: "production" },
          { name: "BUSINESS_READ_API_BASE_URL", value: capacityFixture ? capacityUrl : "https://127.0.0.1:9/" },
          { name: "BUSINESS_TOOL_MAX_PAYLOAD_BYTES", value: payloadLimit },
          {
            name: "BUSINESS_READ_SERVICE_CREDENTIAL",
            value: "0123456789abcdef0123456789abcdef",
          },
          {
            name: "BUSINESS_AGENT_DELEGATION_TOKEN",
            value: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
          },
          { name: "BUSINESS_AGENT_ID", value: "acceptance-agent" },
          { name: "BUSINESS_AGENT_TURN_ID", value: "acceptance-turn" },
          {
            name: "BUSINESS_AGENT_TRACE_ID",
            value: traceId,
          },
          {
            name: "BUSINESS_AUTH_GATEWAY_BASE_URL",
            value: capacityFixture ? capacityUrl : "https://127.0.0.1:9/",
          },
          { name: "BUSINESS_READ_SERVICE_AUTH_MODE", value: "shared_secret" },
          {
            name: "BUSINESS_READ_SERVICE_AUDIENCE",
            value: "business-read-api",
          },
          { name: "BUSINESS_ANOMALY_ENABLED", value: "true" },
          { name: "BUSINESS_AGENT_DRAFT_WRITE_ENABLED", value: "true" },
          { name: "BUSINESS_CHAT_APPROVAL_ENABLED", value: "true" },
          { name: "BUSINESS_AGENT_APPROVAL_SCOPE", value: process.env.BUSINESS_RUNTIME_APPROVAL_SCOPE ?? "" },
        ],
      },
    ],
  });

  const prompt = await request("session/prompt", {
    sessionId: session.sessionId,
    prompt: [
      {
        type: "text",
        text: capacityFixture ? "Run the supplied capacity probe and reply probe complete." : "Reply with probe complete without calling a tool.",
      },
    ],
  });
  assert.equal(prompt.stopReason, "end_turn");
  if (capacityFixture) {
    assert(observedRequests.length >= 2);
    const toolMessage = observedRequests.flatMap(body => body.messages ?? []).find(message => message.role === "tool");
    assert(toolMessage, `model must receive the count result: ${JSON.stringify(observedRequests.map(body => ({keys:Object.keys(body), messages:(body.messages??[]).map(m=>({role:m.role,length:JSON.stringify(m.content).length})), calls:capacityCalls.map(c=>c.path)})))}`);
    assert(!toolMessage.content.includes("elided from tool result"), "count result must not be truncated by the agent text budget");
    const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === "object" ? Object.fromEntries(Object.keys(value).sort().map(key=>[key,canonical(value[key])])) : value;
    const digest = value => createHash("sha256").update(JSON.stringify(canonical(value))).digest("hex");
    assert.equal(digest(JSON.parse(toolMessage.content)), digest(capacityFixture), "all returned preview lines, effects, pagination and the full confirmation hash must survive transport");
    if (capacityPage) {
      const pageMessage=observedRequests.flatMap(body=>body.messages??[]).find(message=>message.role==="tool" && message.tool_call_id==="capacity-page-call");
      assert(pageMessage,"model must receive the requested later preview page");
      const decodedPage=JSON.parse(pageMessage.content);
      assert.equal(decodedPage.status,"ok", JSON.stringify({...decodedPage,items:undefined}));
      assert.equal(digest(decodedPage.items),digest(capacityPage.items));
      assert.equal(decodedPage.pagination.hasMore,capacityPage.pagination.hasMore);
      assert.equal(decodedPage.pagination.nextCursor ?? null,capacityPage.pagination.nextCursor ?? null);
      assert.deepEqual(decodedPage.summary,capacityPage.summary);
      assert.deepEqual(decodedPage.resourceRefs.filter(ref=>ref.type!=="agent_query"),capacityPage.resourceRefs);
      assert.equal(decodedPage.resourceRefs.find(ref=>ref.type==="agent_query")?.bizUri, `biz://agent-query/${traceId}`);
      assert.equal(decodedPage.traceId,capacityPage.traceId);
      assert.deepEqual(capacityCalls.find(call=>call.path==="/v1/read/get_inventory_count_approval_preview")?.input,capacityPageInput);
    }
    const write = capacityCalls.find(call => call.path === "/v1/write/prepare_inventory_count_submission");
    assert.deepEqual(write?.input, capacityInput);
    assert.equal(capacityCalls.filter(call => call.path === "/v1/write/prepare_inventory_count_submission").length, 1);
  } else { assert.equal(observedRequests.length, 1); }

  const modelTools = observedRequests[0].tools
    .map((tool) => tool.function?.name ?? tool.name)
    .filter(Boolean)
    .sort();
  const approvalScope = process.env.BUSINESS_RUNTIME_APPROVAL_SCOPE;
  const approvalTool = approvalScope ? "approve_" + approvalScope.replace(/:approve$/, "").replace(/_intent$/, "") : null;
  const expectedVisible = expectedTools.filter(qualifiedName => {
    const name = qualifiedName.replace(/^business-read-mcp__/, "");
    return approvalTool
      ? name === approvalTool || !/^(approve|prepare|create|update)_/.test(name)
      : !name.startsWith("approve_");
  });
  assert.deepEqual(modelTools, expectedVisible.sort());
  assert(modelTools.length <= 128);

  console.log(
    JSON.stringify({
      runtime: "buzz-agent",
      mcp: "business-read-mcp",
      modelVisibleTools: modelTools.length,
      onlyFixedBusinessTools: true,
      promptCompleted: true,
      ...(capacityFixture ? { laterPreviewPageReachedModel: Boolean(capacityPage), capacityInputLines: capacityInput.command.lines.length, capacityPayloadBytes: Buffer.byteLength(JSON.stringify(capacityFixture)), completeReturnedPreviewReachedModel: true, totalCountLines: capacityFixture.previewPagination?.totalLines, returnedPreviewLines: capacityFixture.document.lines.length, mockedLocalServices: true, contextTokenLimit: Number(contextLimit), payloadByteLimit: Number(payloadLimit), toolTextByteLimit: Number(textLimit) } : {}),
    }),
  );
} finally {
  clearTimeout(timeout);
  lines.close();
  agent.kill("SIGTERM");
  await Promise.all([new Promise((resolve) => modelServer.close(resolve)), new Promise(resolve => capacityServer.close(resolve))]);
}
