import type { ReactNode } from "react";
import { X } from "lucide-react";

export function DialogHeader({
  id,
  title,
  subtitle,
  closeLabel,
  onClose,
  disabled = false,
}: {
  id: string;
  title: ReactNode;
  subtitle?: string;
  closeLabel: string;
  onClose: () => void;
  disabled?: boolean;
}) {
  return (
    <header className="dialog-head dialog-head-dismissible">
      <button
        type="button"
        className="icon-button"
        aria-label={closeLabel}
        disabled={disabled}
        onClick={onClose}
      >
        <X size={14} aria-hidden="true" />
      </button>
      <div>
        <h2 id={id}>{title}</h2>
        {subtitle && <p className="dialog-subtitle">{subtitle}</p>}
      </div>
    </header>
  );
}
