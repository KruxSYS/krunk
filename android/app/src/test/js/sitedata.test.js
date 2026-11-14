// Tests for the site-data extension's plain part (assets/extensions/krunk-sitedata/sitedata.js),
// run with `node --test` by the Gradle task siteDataJsTest (part of testDebugUnitTest).
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const S = require(path.join(__dirname, "../../main/assets/extensions/krunk-sitedata/sitedata.js"));

test("cookies are counted per host, without the leading dot", () => {
  const rows = S.cookieHosts([
    { domain: ".news.example", partitionKey: null },
    { domain: "news.example", partitionKey: null },
    { domain: "WWW.news.example" },
    { domain: "10.0.2.2", partitionKey: null },
  ]);
  assert.deepEqual(rows, [
    ["10.0.2.2", 1],
    ["news.example", 2],
    ["www.news.example", 1],
  ]);
});

test("a cookie kept apart under a site is counted for that site", () => {
  const rows = S.cookieHosts([
    { domain: ".tracker.test", partitionKey: { topLevelSite: "https://news.example", hasCrossSiteAncestor: true } },
    { domain: "tracker.test", partitionKey: { topLevelSite: "http://shop.example:8080" } },
    { domain: "tracker.test", partitionKey: null },
    // A partition key the engine gave no site for: the cookie's own host.
    { domain: "odd.test", partitionKey: {} },
  ]);
  assert.deepEqual(rows, [
    ["news.example", 1],
    ["odd.test", 1],
    ["shop.example", 1],
    ["tracker.test", 1],
  ]);
});

test("what is not a cookie is skipped", () => {
  assert.deepEqual(S.cookieHosts(null), []);
  assert.deepEqual(S.cookieHosts([null, {}, { domain: "." }, { domain: 7 }]), []);
});

test("partition keys in the engine's formats", () => {
  assert.equal(S.hostOfPartitionKey(""), "");
  assert.equal(S.hostOfPartitionKey(undefined), "");
  assert.equal(S.hostOfPartitionKey("(https,news.example)"), "news.example");
  assert.equal(S.hostOfPartitionKey("(http,shop.example,8080)"), "shop.example");
  assert.equal(S.hostOfPartitionKey("(https,news.example,f)"), "news.example");
  assert.equal(S.hostOfPartitionKey("(https,news.example,8443,f)"), "news.example");
  assert.equal(S.hostOfPartitionKey("old.example"), "old.example");
  assert.equal(S.hostOfSite("https://[::1]:8080"), "[::1]");
  assert.equal(S.hostOfSite("not a site"), "");
});

test("stored data per origin and the site it is kept under", () => {
  const rows = S.usageRows([
    { origin: "https://www.news.example", partitionKey: "", bytes: 1000 },
    { origin: "https://tracker.test", partitionKey: "(https,news.example)", bytes: 40.7 },
    { origin: "https://tracker.test", partitionKey: "(https,news.example,f)", bytes: 2 },
    { origin: "https://tracker.test", partitionKey: "", bytes: 5 },
    { origin: "moz-extension://abc", partitionKey: "", bytes: 9 },
    { origin: "http://10.0.2.2:18080", bytes: -1 },
    null,
  ]);
  assert.deepEqual(rows, [
    ["https://www.news.example", "", 1000],
    ["https://tracker.test", "news.example", 42],
    ["https://tracker.test", "", 5],
    ["http://10.0.2.2:18080", "", 0],
  ]);
  assert.deepEqual(S.usageRows("nothing"), []);
});
