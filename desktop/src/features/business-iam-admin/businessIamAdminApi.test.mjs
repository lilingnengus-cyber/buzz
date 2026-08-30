import assert from "node:assert/strict";
import test from "node:test";

import {
  BusinessIamApiError,
  describeIamError,
} from "./businessIamAdminApi.ts";

test("describes known errors without exposing response bodies", () => {
  assert.equal(
    describeIamError(new BusinessIamApiError(409, "approver_already_decided")),
    "You already reviewed this change.",
  );
  assert.match(
    describeIamError(new BusinessIamApiError(400, "unknown_code")),
    /unknown_code/,
  );
});
