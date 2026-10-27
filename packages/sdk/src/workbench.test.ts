import cases from "../../../contracts/workbench-v4.examples.json";
import { expect, it } from "vitest";
import { validateWorkbenchRequest,validateWorkbenchResult,validateWorkbenchEvent } from "./workbench";
it("checks the shared workbench positive and negative examples", () => {
  for (const item of cases) {const validate=item.kind==='result'?validateWorkbenchResult:item.kind==='event'?validateWorkbenchEvent:validateWorkbenchRequest;expect(validate(item.method,item.params)===undefined,JSON.stringify(item)).toBe(item.valid);}
});
