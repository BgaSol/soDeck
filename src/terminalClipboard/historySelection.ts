/** Read only selected record text, excluding timestamps and row controls. */
export function historySelection(root: HTMLElement): string {
  const selection = root.ownerDocument.getSelection();
  if (
    !selection?.rangeCount ||
    selection.isCollapsed ||
    !root.contains(selection.anchorNode) ||
    !root.contains(selection.focusNode)
  )
    return "";
  const selected = selection.getRangeAt(0);
  const parts: string[] = [];
  for (const element of root.querySelectorAll(".history-row-content")) {
    if (!selected.intersectsNode(element)) continue;
    const part = root.ownerDocument.createRange();
    part.selectNodeContents(element);
    if (selected.compareBoundaryPoints(Range.START_TO_START, part) > 0)
      part.setStart(selected.startContainer, selected.startOffset);
    if (selected.compareBoundaryPoints(Range.END_TO_END, part) < 0)
      part.setEnd(selected.endContainer, selected.endOffset);
    const text = part.toString();
    if (text) parts.push(text);
  }
  return parts.join("\n");
}
