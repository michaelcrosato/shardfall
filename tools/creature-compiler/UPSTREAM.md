# SpawnForge compiler pin

This package embeds the production TypeScript source from
[SpawnForge](https://github.com/michaelcrosato/spawnforge) at commit
`851880256987ecdb2895c6afd01f84df64199bdb`.

`vendor/core/src` and `vendor/modules/src` are exact source copies. Upstream tests,
apps, renderers, binary assets, and export backends are not part of this package.
`pin.json` records a SHA-256 for each copied source file. Package manifests only
replace the upstream workspace/catalog dependency syntax with exact npm pins.
The upstream packages are private and marked UNLICENSED. That metadata is retained.
This integration is for the repository owner's Shardfall and SpawnForge projects.
It does not assign a new license to SpawnForge.

The Shardfall adapter is in `src/engine.mjs`. Its native data format is version 1.
It keeps model-space bind bones, local animation tracks, linear vertex colors,
four bone influences per vertex, and double-sided membranes. Source blueprints
remain readable JSON. The compiled JSON is a disposable local cache.

Build with Node 22.18 or newer:

```sh
npm ci
npm run build
npm test
```

The generated `dist/worker.mjs` is self-contained. A release package needs this
bundle, `worker.mjs`, `catalog.json`, `blueprint.schema.json`, `templates`, this
notice, `pin.json`, and `dist/THIRD_PARTY_NOTICES.txt`. It does not need npm or
node_modules. The Windows package includes a portable Node runtime and its license.

To update the pin, recopy exact sources, update pin.json and the native validator's
compiler revision, rebuild the catalogs, and run the compiler/native contract tests.
Never silently point this adapter at a newer SpawnForge checkout.
