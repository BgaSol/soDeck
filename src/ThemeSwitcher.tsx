import { useSyncExternalStore } from "react";
import { Moon, Sun } from "lucide-react";
import { getTheme, setTheme, subscribeTheme } from "./theme";

export function ThemeSwitcher() {
  const theme = useSyncExternalStore(subscribeTheme, getTheme);
  return (
    <div className="theme-switcher" role="group" aria-label="外观主题">
      {(
        [
          ["light", "白天主题", Sun],
          ["dark", "黑夜主题", Moon],
        ] as const
      ).map(([value, label, Icon]) => (
        <button
          key={value}
          type="button"
          aria-label={label}
          title={label}
          aria-pressed={theme === value}
          onClick={() => setTheme(value)}
        >
          <Icon size={15} aria-hidden="true" />
        </button>
      ))}
    </div>
  );
}
