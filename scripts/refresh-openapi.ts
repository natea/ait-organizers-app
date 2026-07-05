/**
 * Fetch the canonical AI Tinkerers OpenAPI contract and regenerate typed
 * client definitions. The docs instruct consumers to fetch from the canonical
 * URL rather than vendor a stale copy (docs/agents-api.md), so this script is
 * the single refresh point. Run with: `bun run refresh-openapi`.
 */
const SPEC_URL = "https://aitinkerers.org/api/agents/v1/openapi.yaml";
const OUT_YAML = "openapi/openapi.yaml";
const OUT_TYPES = "src/generated/api-types.ts";

async function main(): Promise<void> {
  console.log(`Fetching ${SPEC_URL} …`);
  const res = await fetch(SPEC_URL);
  if (!res.ok) {
    console.error(`Failed to fetch spec: HTTP ${res.status}`);
    process.exit(1);
  }
  const yaml = await res.text();
  await Bun.write(OUT_YAML, yaml);
  console.log(`Vendored spec → ${OUT_YAML} (${yaml.length} bytes)`);

  // Generate TypeScript types from the vendored spec.
  const proc = Bun.spawn(
    ["bunx", "--bun", "openapi-typescript", OUT_YAML, "-o", OUT_TYPES],
    { stdout: "inherit", stderr: "inherit" },
  );
  const code = await proc.exited;
  if (code === 0) {
    console.log(`Generated types → ${OUT_TYPES}`);
  } else {
    console.warn(
      `openapi-typescript exited ${code}. Spec is vendored; run the generator manually if needed.`,
    );
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
