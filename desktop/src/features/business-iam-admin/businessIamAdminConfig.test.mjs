import assert from "node:assert/strict";
import test from "node:test";

import { readBusinessIamAdminConfig } from "./businessIamAdminConfig.ts";

test("accepts production HTTPS and development loopback origins", () => {
  assert.deepEqual(readBusinessIamAdminConfig({}), {
    config: null,
    error: null,
  });
  assert.equal(
    readBusinessIamAdminConfig({
      VITE_BUSINESS_IAM_ADMIN_URL: "https://iam.example.com/",
    }).config?.baseUrl,
    "https://iam.example.com",
  );
  assert.equal(
    readBusinessIamAdminConfig({
      VITE_BUSINESS_IAM_ADMIN_URL: "http://127.0.0.1:3110/",
    }).config?.baseUrl,
    "http://127.0.0.1:3110",
  );
});

test("rejects insecure remote, credentialed, and path URLs", () => {
  for (const value of [
    "http://iam.example.com/",
    "https://user:password@iam.example.com/",
    "https://iam.example.com/api",
  ]) {
    assert.equal(
      readBusinessIamAdminConfig({
        VITE_BUSINESS_IAM_ADMIN_URL: value,
      }).config,
      null,
    );
  }
});
