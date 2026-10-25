import { Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { THEME_PREFERENCES, type ThemePreference } from "@/theme/theme";

export const THEME_OPTIONS: { value: ThemePreference; label: string; icon: LucideIcon }[] = THEME_PREFERENCES.map(
  (value) => ({
    value,
    label: { light: "Light", dark: "Dark", system: "System" }[value],
    icon: { light: Sun, dark: Moon, system: Monitor }[value],
  }),
);

export function isThemePreference(v: string): v is ThemePreference {
  return (THEME_PREFERENCES as readonly string[]).includes(v);
}
