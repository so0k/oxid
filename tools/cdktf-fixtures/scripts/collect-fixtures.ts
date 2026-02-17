/**
 * Collects synthesized tf.json files from all CDKTF packages
 * and copies them into a unified fixtures directory for Oxid tests.
 *
 * Usage: bun run scripts/collect-fixtures.ts
 * Output: ../../../tests/fixtures/tf-json/{foreach,modules,multi-provider}/cdk.tf.json
 */
import { existsSync, mkdirSync, cpSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const PACKAGES_DIR = join(ROOT, "packages");
const FIXTURES_DIR = resolve(ROOT, "..", "..", "tests", "fixtures", "tf-json");

const packages = readdirSync(PACKAGES_DIR, { withFileTypes: true })
  .filter((d) => d.isDirectory())
  .map((d) => d.name);

console.log(`Collecting fixtures from ${packages.length} packages...`);

for (const pkg of packages) {
  const synthDir = join(PACKAGES_DIR, pkg, "cdktf.out", "stacks");

  if (!existsSync(synthDir)) {
    console.warn(`  SKIP ${pkg}: no cdktf.out/stacks/ (run synth first)`);
    continue;
  }

  // Each stack produces a directory under stacks/
  const stacks = readdirSync(synthDir, { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => d.name);

  for (const stack of stacks) {
    const srcFile = join(synthDir, stack, "cdk.tf.json");
    if (!existsSync(srcFile)) {
      console.warn(`  SKIP ${pkg}/${stack}: no cdk.tf.json`);
      continue;
    }

    const destDir = join(FIXTURES_DIR, pkg);
    mkdirSync(destDir, { recursive: true });

    const destFile = join(destDir, "cdk.tf.json");
    cpSync(srcFile, destFile);
    console.log(`  OK   ${pkg}/${stack} → fixtures/tf-json/${pkg}/cdk.tf.json`);
  }
}

console.log(`\nFixtures written to: ${FIXTURES_DIR}`);
