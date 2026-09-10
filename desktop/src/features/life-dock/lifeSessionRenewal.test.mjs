import assert from "node:assert/strict";
import test from "node:test";
import { watchLifeSessionRenewal } from "./lifeSessionRenewal.ts";

function fixture() {
  const host = new EventTarget();
  host.document = new EventTarget();
  host.document.visibilityState = "hidden";
  host.setTimeout = (fn, delay) => {
    host.timer = fn;
    host.delay = delay;
    return 1;
  };
  host.clearTimeout = () => {
    host.timer = null;
  };
  return host;
}

test("renews with the panel hidden and coalesces wake events", (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 0 });
  const host = fixture();
  let calls = 0;
  const stop = watchLifeSessionRenewal(
    new Date(3600000).toISOString(),
    () => calls++,
    host,
  );
  assert.equal(host.delay, 3510000);
  host.dispatchEvent(new Event("focus"));
  assert.equal(calls, 0);
  t.mock.timers.setTime(3510000);
  host.timer();
  host.dispatchEvent(new Event("online"));
  assert.equal(calls, 1);
  stop();
  assert.equal(host.timer, null);
});

test("waking after expiry checks immediately without waiting for a suspended timer", (t) => {
  t.mock.timers.enable({ apis: ["Date"], now: 0 });
  const host = fixture();
  let calls = 0;
  const stop = watchLifeSessionRenewal(
    new Date(3600000).toISOString(),
    () => calls++,
    host,
  );
  t.mock.timers.setTime(7200000);
  host.document.visibilityState = "visible";
  host.document.dispatchEvent(new Event("visibilitychange"));
  host.dispatchEvent(new Event("focus"));
  assert.equal(calls, 1);
  stop();
  host.dispatchEvent(new Event("online"));
  assert.equal(calls, 1);
});

test("cleanup prevents renewal after logout or unmount", () => {
  const host = fixture();
  let calls = 0;
  const stop = watchLifeSessionRenewal(
    new Date(0).toISOString(),
    () => calls++,
    host,
  );
  const pendingTimer = host.timer;
  stop();
  pendingTimer();
  host.dispatchEvent(new Event("focus"));
  assert.equal(calls, 0);
});
