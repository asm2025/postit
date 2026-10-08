// TypeScript 7 (the native compiler) ships no classic JS API, which typescript-eslint and
// openapi-typescript need. Those packages get TypeScript 6's API through its official
// side-by-side package, as a direct dependency instead of a peer (a peer would resolve to
// the project's TypeScript 7). The project's own `tsc` stays TypeScript 7.
const TS6 = 'npm:@typescript/typescript6@6.0.2'

const NEEDS_TS_API = new Set(['typescript-eslint', 'ts-api-utils', 'openapi-typescript'])

function needsTsApi(name) {
  return NEEDS_TS_API.has(name) || name.startsWith('@typescript-eslint/')
}

function readPackage(pkg) {
  if (!needsTsApi(pkg.name)) return pkg
  if (pkg.peerDependencies?.typescript) delete pkg.peerDependencies.typescript
  if (pkg.peerDependenciesMeta?.typescript) delete pkg.peerDependenciesMeta.typescript
  pkg.dependencies = { ...pkg.dependencies, typescript: TS6 }
  return pkg
}

module.exports = { hooks: { readPackage } }
