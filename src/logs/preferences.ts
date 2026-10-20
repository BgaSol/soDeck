import { useEffect, useState } from "react";
const key = "sodeck.log-display.v1";
export interface LogPreferences {
  timestamps: boolean;
  wrap: boolean;
  highlight: boolean;
  fontSize: number;
}
const defaults: LogPreferences = {
  timestamps: true,
  wrap: false,
  highlight: true,
  fontSize: 13,
};
function load(): LogPreferences {
  try {
    const v = JSON.parse(localStorage.getItem(key) || "null");
    return {
      timestamps: typeof v?.timestamps === "boolean" ? v.timestamps : true,
      wrap: typeof v?.wrap === "boolean" ? v.wrap : false,
      highlight: typeof v?.highlight === "boolean" ? v.highlight : true,
      fontSize: Number.isFinite(v?.fontSize)
        ? Math.min(22, Math.max(11, v.fontSize))
        : 13,
    };
  } catch {
    return defaults;
  }
}
export function useLogPreferences() {
  const [value, setValue] = useState(load);
  useEffect(() => {
    const refresh = () => setValue(load());
    window.addEventListener("storage", refresh);
    window.addEventListener("focus", refresh);
    return () => {
      window.removeEventListener("storage", refresh);
      window.removeEventListener("focus", refresh);
    };
  }, []);
  return [
    value,
    (patch: Partial<LogPreferences>) =>
      setValue((previous) => {
        const next = { ...previous, ...patch };
        try {
          localStorage.setItem(key, JSON.stringify(next));
        } catch {
          /* The view remains usable if persistence is unavailable. */
        }
        return next;
      }),
  ] as const;
}
