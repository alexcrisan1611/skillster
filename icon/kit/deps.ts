// The kit's two libraries, installed once into a cache outside any project, so using the kit never adds a
// dependency to the app: @resvg/resvg-js renders SVG, pngjs reads reference PNGs for measure.ts.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { homedir } from "node:os";
import { join } from "node:path";

const CACHE = join(homedir(), ".cache", "solar-app-icon");
const PACKAGES = ["@resvg/resvg-js@2.6.2", "pngjs@7.0.0"];

export function load<T = any>(name: "@resvg/resvg-js" | "pngjs"): T {
  if (!existsSync(join(CACHE, "node_modules", name))) {
    mkdirSync(CACHE, { recursive: true });
    if (!existsSync(join(CACHE, "package.json"))) writeFileSync(join(CACHE, "package.json"), '{ "private": true }\n');
    const run = Bun.spawnSync(["bun", "add", "--exact", ...PACKAGES], { cwd: CACHE, stdout: "inherit", stderr: "inherit" });
    if (run.exitCode !== 0) throw new Error(`could not install ${PACKAGES.join(", ")} into ${CACHE}`);
  }
  return createRequire(join(CACHE, "package.json"))(name);
}
