# Embedded action delete confirmation

The action delete button used window.confirm, which depends on browser modal
support in the embedding webview. A denied or unsupported confirm returns without
a delete request and without any visible feedback.

LifeOS commit 56b243346bf3dc2c32c865dafc097ce4b64d191e replaces that dependency
with an in-page HTML dialog in a body portal. The existing trigger remains;
only explicit confirmation calls the existing DELETE API. Cancel/Escape close
without writing. During a request the controls are disabled, duplicate calls
are guarded, and failures stay visible in the dialog for retry.

Validation: TypeScript check passed. A real browser test mounts the actual
component inside a sandboxed iframe without allow-modals, using a mocked DELETE
API. It covers opening without a write, cancel, Escape, failure display, retry,
the exact target URL, and the successful deletion callback. No production data
is deleted by these tests.

The change stays within LifeOS's native action UI; chat deletion approval is
unchanged. Production deployment succeeded in Actions run 34078888169 (2m17s).
After refreshing the Life Dock in the installed Pacioli app, opening action
cmtqmxutb0009wmj2nkf1es6n and clicking its delete button displays the in-page
confirmation with the correct title 核对完整回执. Focus starts on Cancel.
Clicking Cancel closes confirmation and retains the action detail. No production
DELETE was submitted; the success/error API paths were covered with the mocked
browser regression test.
