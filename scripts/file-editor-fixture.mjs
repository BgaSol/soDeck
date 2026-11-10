// Both shipped editor generations remain usable by the native package harness.
export function fileDocument(frame) {
  const host=frame.locator('.code-editor'),legacy=frame.locator('textarea[aria-label="远端文件编辑器"]');
  const modern=async()=>{await host.or(legacy).first().waitFor();return await host.count()>0;};
  return {
    waitFor:()=>host.or(legacy).first().waitFor(),
    isEnabled:async()=>await modern()?host.evaluate(el=>el.dataset.ready==='true'&&!el.sodeckEditor.isReadOnly()):legacy.isEnabled(),
    inputValue:async()=>await modern()?host.evaluate(el=>el.sodeckEditor?.value()):legacy.inputValue(),
    fill:async value=>await modern()?host.evaluate((el,value)=>el.sodeckEditor.setText(value),value):legacy.fill(value),
    evaluate:(fn,arg)=>host.evaluate(fn,arg),
  };
}
