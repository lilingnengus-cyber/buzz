import assert from "node:assert/strict";
import test from "node:test";
import {
  ApiRequestError,
  apiErrorMessage,
  isUnavailableResourceError,
  toApiFailure,
} from "./api.ts";

test("localizes the deliberately opaque authorization miss", () => {
  assert.equal(
    apiErrorMessage(
      {
        code: "not_found_or_forbidden",
        message: "resource was not found or is not accessible",
      },
      404,
    ),
    "当前账号无法访问此资源",
  );
});

test("classifies page failures for recovery UI", () => {
  assert.equal(
    toApiFailure(new ApiRequestError({ status: 403, message: "forbidden" }))
      .kind,
    "access_denied",
  );
  assert.equal(
    toApiFailure(
      new ApiRequestError({ status: 401, message: "session expired" }),
    ).kind,
    "session_expired",
  );
  assert.equal(
    toApiFailure(
      new ApiRequestError({ status: 503, message: "service unavailable" }),
    ).kind,
    "service_unavailable",
  );
  assert.equal(
    toApiFailure(new TypeError("fetch failed")).kind,
    "service_unavailable",
  );
});

test("classifies an opaque authorization miss without exposing existence", () => {
  const error = new ApiRequestError({
    status: 404,
    code: "not_found_or_forbidden",
    message: "当前账号无法访问此资源",
    traceId: "trace-1",
  });

  assert.equal(isUnavailableResourceError(error), true);
  assert.equal(isUnavailableResourceError(new Error(error.message)), false);
  assert.equal(error.traceId, "trace-1");
});

test("preserves a specific server message", () => {
  assert.equal(
    apiErrorMessage(
      { code: "invalid_request", message: "订单版本已变化" },
      409,
    ),
    "订单版本已变化",
  );
});

test("falls back to the response status for an empty body", () => {
  assert.equal(apiErrorMessage({}, 503), "请求失败（503）");
});
