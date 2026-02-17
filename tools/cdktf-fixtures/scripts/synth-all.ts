/**
 * Synthesizes all CDKTF packages by running `bun run main.ts` in each.
 * CDKTF's app.synth() writes tf.json to cdktf.out/stacks/<stack>/.
 *
 * Usage: bun run scripts/synth-all.ts
 */
import { readdirSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { $ } from "bun";

const ROOT = resolve(import.meta.dir, "..");
const PACKAGES_DIR = join(ROOT, "packages");

const packages = readdirSync(PACKAGES_DIR, { withFileTypes: true })
  .filter((d) => d.isDirectory() && existsSync(join(PACKAGES_DIR, d.name, "main.ts")))
  .map((d) => d.name)
  .sort();

console.log(`Synthesizing ${packages.length} packages...\n`);

let passed = 0;
let failed = 0;

for (const pkg of packages) {
  const pkgDir = join(PACKAGES_DIR, pkg);
  process.stdout.write(`  ${pkg} ... `);
  try {
    await $`bun run main.ts`.cwd(pkgDir).quiet();
    console.log("OK");
    passed++;
  } catch (err: any) {
    console.log("FAILED");
    console.error(`    ${err.stderr?.toString().trim() || err.message}\n`);
    failed++;
  }
}

console.log(`\nDone: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
