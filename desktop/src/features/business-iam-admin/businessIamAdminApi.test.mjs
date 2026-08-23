import assert from "node:assert/strict";
import test from "node:test";

import {
  BusinessIamApiError,
  describeIamError,
  isStepUpRequired,
} from "./businessIamAdminApi.ts";

test("recognizes only explicit Step-up failures", () => {
  assert.equal(
    isStepUpRequired(new BusinessIamApiError(403, "step_up_expired")),
    true,
  );
  assert.equal(
    isStepUpRequired(
      new BusinessIamApiError(403, "business_iam_permission_denied"),
    ),
    false,
  );
});

test("describes known errors without exposing response bodies", () => {
  assert.equal(
    describeIamError(new BusinessIamApiError(403, "requester_cannot_approve")),
    "The requester cannot approve their own change.",
  );
  assert.match(
    describeIamError(new BusinessIamApiError(400, "unknown_code")),
    /unknown_code/,
  );
});
