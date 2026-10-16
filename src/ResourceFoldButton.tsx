import { ChevronDown, ChevronRight } from "lucide-react";
export interface ResourceFoldProps {
  collapsed?: boolean;
  onToggle?: () => void;
}
export function ResourceFoldButton({
  label,
  collapsed,
  onToggle,
}: ResourceFoldProps & { label: string }) {
  return (
    <button
      className="resource-fold"
      aria-label={`${collapsed ? "展开" : "折叠"}${label}`}
      aria-expanded={!collapsed}
      onClick={onToggle}
    >
      {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
    </button>
  );
}
