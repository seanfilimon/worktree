import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  readThemePreference,
  resolveTheme,
  systemPrefersDark,
  ThemeContext,
  themeStore,
  type ThemePreference,
} from "./theme";

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [preference, setPreferenceState] = useState<ThemePreference>(readThemePreference);
  const [prefersDark, setPrefersDark] = useState(systemPrefersDark);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setPrefersDark(e.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  const resolved = resolveTheme(preference, prefersDark);

  useEffect(() => {
    document.documentElement.dataset.theme = resolved;
    document.documentElement.classList.toggle("dark", resolved === "dark");
  }, [resolved]);

  const setPreference = useCallback((pref: ThemePreference) => {
    themeStore.set(pref);
    setPreferenceState(pref);
  }, []);

  const value = useMemo(() => ({ preference, resolved, setPreference }), [preference, resolved, setPreference]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}
