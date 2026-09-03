import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "utils", "usage.ts");

async function loadUsageModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(
    source
      .replace(
        'import { formatLocalDateTimeFromSeconds } from "@/lib/utils/time";',
        'const formatLocalDateTimeFromSeconds = (timestamp, emptyLabel) => emptyLabel || String(timestamp || "");',
      )
      .replace('import { Account, AccountUsage, AvailabilityLevel, RequestLog } from "@/types";', ""),
    {
      compilerOptions: {
        module: ts.ModuleKind.ES2022,
        target: ts.ScriptTarget.ES2022,
      },
      fileName: sourcePath,
    },
  );

  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-luna-reserve-"));
  const tempFile = path.join(tempDir, "usage.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const usage = await loadUsageModule();

const reserveCreditsJson = JSON.stringify({
  additionalRateLimits: [
    {
      limitName: "Luna Reserve",
      meteredFeature: "base_model_inference",
      allowed: true,
      limitReached: false,
      rateLimit: {
        primaryWindow: {
          remainingPercent: 80,
          limitWindowSeconds: 604800,
        },
      },
    },
  ],
});

test("Luna Reserve 的 camelCase 用量会显示且保持可用", () => {
  const snapshot = {
    usedPercent: 100,
    secondaryUsedPercent: 100,
    creditsJson: reserveCreditsJson,
  };

  assert.equal(usage.hasUsableLunaReserve(snapshot), true);
  assert.deepEqual(usage.calcAvailability(snapshot, { status: "active" }), {
    text: "仅 Luna Reserve",
    level: "ok",
  });
  const rows = usage.getExtraUsageDisplayRows(snapshot);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].label, "Luna Reserve");
  assert.equal(rows[0].remainPercent, 80);
});

test("强制开启状态绕过额度状态并默认关闭", () => {
  assert.deepEqual(usage.calcAvailability(undefined, { status: "force_enabled" }), {
    text: "强制开启",
    level: "ok",
  });
  assert.deepEqual(usage.calcAvailability(undefined, { status: "active" }), {
    text: "未知",
    level: "unknown",
  });
});

test("明确耗尽的 Luna Reserve 不会被当作可用额度", () => {
  const exhausted = {
    usedPercent: 100,
    creditsJson: JSON.stringify({
      additionalRateLimits: [
        {
          limitName: "Luna Reserve",
          limitReached: true,
          rateLimit: { primaryWindow: { remainingPercent: 100 } },
        },
      ],
    }),
  };
  assert.equal(usage.hasUsableLunaReserve(exhausted), false);
  assert.deepEqual(usage.calcAvailability(exhausted, { status: "limited" }), {
    text: "限流",
    level: "bad",
  });
});
