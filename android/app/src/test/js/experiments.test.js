// krunk-guard's parent experiment scripts share ONE global scope in Gecko (every script listed
// under "experiment_apis" with a "parent" part is loaded into the same sandbox). A top-level
// const declared twice throws at load and takes the whole guard down; a function declared twice
// silently replaces the other script's. 0.10.0 shipped to the emulator with exactly that
// ("redeclaration of const stateJson"): this test loads them together, as the engine does.
"use strict";
const test = require("node:test");
const assert = require("node:assert");
const fs = require("fs");
const path = require("path");
const vm = require("vm");

const dir = path.join(__dirname, "../../main/assets/extensions/krunk-guard");
const manifest = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));

test("all parent experiment scripts load together and each defines its API", () => {
  const parents = Object.entries(manifest.experiment_apis).filter(([, api]) => api.parent);
  assert.ok(parents.length >= 2, "gate and vpn");
  const ctx = { ExtensionAPI: class {}, Services: {}, Cc: {}, Ci: {}, Cr: {}, ChromeUtils: {} };
  vm.createContext(ctx);
  for (const [, api] of parents) {
    const file = path.join(dir, api.parent.script);
    vm.runInContext(fs.readFileSync(file, "utf8"), ctx, { filename: file });
  }
  for (const [name] of parents) assert.equal(typeof ctx[name], "function", name);
});

