import {test} from 'node:test';
import assert from 'node:assert/strict';
import {boundaryErrors} from './dependency-boundaries.mjs';
test('negative fixtures reject cross-plugin, shell and business imports',()=>{
  for(const source of ['import x from "../../terminal/src/worker"','export {x} from "../../../apps/desktop/src/app"','const x=import("@sodeck/terminal")','const x=require("@tauri-apps/api")'])assert.equal(boundaryErrors('plugins/files/src/worker.ts',source).length,1,source);
  assert.equal(boundaryErrors('crates/core/src/lib.rs','mod files_plugin;').length,1);
  assert.equal(boundaryErrors('crates/core/src/lib.rs','mod mounts;').length,1);
  assert.deepEqual(boundaryErrors('plugins/files/src/worker.ts','import x from "@sodeck/sdk/remote-read"; import {n} from "./state";'),[]);
});

test('UI and SDK retain dependency direction',()=>{assert.equal(boundaryErrors('packages/ui/src/index.tsx','import {core} from "@sodeck/sdk"').length,1);assert.equal(boundaryErrors('packages/sdk/src/index.ts','import React from "react"').length,1);assert.equal(boundaryErrors('packages/sdk/src/index.ts','import {Button} from "@sodeck/ui"').length,1);});
