import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import React from "react";
import { JSDOM } from "jsdom";

import { createWorkspaceDockRegistry } from "./WorkspaceDockRegistry.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const component = () => React.createElement("div");
const extension = (id, scheme, origin, homeUrl) => ({
  id,
  title: id,
  scheme,
  origin,
  homeUrl,
  resolveResource: () => null,
  Provider: ({ children }) => children,
  Dock: component,
  TopChromeAction: component,
});

test("registry orders valid dock registrations deterministically", () => {
  const registry = createWorkspaceDockRegistry([
    extension(
      "life",
      "life",
      "https://life.example.com",
      "https://life.example.com/embed/",
    ),
    extension(
      "business",
      "biz",
      "https://biz.example.com",
      "https://biz.example.com/embed/",
    ),
  ]);
  assert.deepEqual(
    registry.extensions.map((item) => item.id),
    ["business", "life"],
  );
});

test("registry rejects duplicate ids, duplicate schemes, and unsafe origins", () => {
  assert.throws(() =>
    createWorkspaceDockRegistry([
      extension("business", "biz", null, null),
      extension("business", "life", null, null),
    ]),
  );
  assert.throws(() =>
    createWorkspaceDockRegistry([
      extension("business", "biz", null, null),
      extension("life", "biz", null, null),
    ]),
  );
  assert.throws(() =>
    createWorkspaceDockRegistry([
      extension(
        "life",
        "life",
        "https://life.example.com/path",
        "https://life.example.com/embed/",
      ),
    ]),
  );
  assert.throws(() =>
    createWorkspaceDockRegistry([
      extension(
        "life",
        "life",
        "https://life.example.com",
        "https://other.example.com/embed/",
      ),
    ]),
  );
});

test("an explicitly unconfigured dock remains registered without a web origin", () => {
  const registry = createWorkspaceDockRegistry([
    extension("business", "biz", null, null),
  ]);
  assert.equal(registry.extensions[0].id, "business");
});

test("switching the active workspace keeps every dock mounted", async () => {
  const { fireEvent, render, screen } = await import("@testing-library/react");
  const {
    WorkspaceDockHost,
    WorkspaceDockHostProvider,
    useWorkspaceDockHost,
    useWorkspaceDockSlot,
  } = await import("./WorkspaceDockHost.tsx");
  const mounted = { business: 0, life: 0 };
  const unmounted = { business: 0, life: 0 };
  const Dock = ({ id }) => {
    const slot = useWorkspaceDockSlot(id);
    React.useEffect(() => {
      mounted[id] += 1;
      return () => {
        unmounted[id] += 1;
      };
    }, [id]);
    return React.createElement("p", null, `${id}:${slot.active}`);
  };
  const testExtension = (id, scheme) => ({
    ...extension(id, scheme, null, null),
    Dock: () => React.createElement(Dock, { id }),
  });
  const Controller = () => {
    const host = useWorkspaceDockHost();
    return React.createElement(
      React.Fragment,
      null,
      React.createElement(
        "button",
        {
          onClick: () => host.requestActivation("business"),
          type: "button",
        },
        "business",
      ),
      React.createElement(
        "button",
        { onClick: () => host.requestActivation("life"), type: "button" },
        "life",
      ),
      React.createElement(WorkspaceDockHost),
    );
  };

  render(
    React.createElement(
      WorkspaceDockHostProvider,
      {
        extensions: [
          testExtension("business", "biz"),
          testExtension("life", "life"),
        ],
      },
      React.createElement(Controller),
    ),
  );
  assert.ok(screen.getByText("business:false"));
  assert.ok(screen.getByText("life:false"));

  fireEvent.click(screen.getByRole("button", { name: "business" }));
  assert.ok(screen.getByText("business:true"));
  fireEvent.click(screen.getByRole("button", { name: "life" }));
  assert.ok(screen.getByText("business:false"));
  assert.ok(screen.getByText("life:true"));
  assert.deepEqual(mounted, { business: 1, life: 1 });
  assert.deepEqual(unmounted, { business: 0, life: 0 });
});
