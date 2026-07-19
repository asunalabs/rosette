"use client";

import { createContext, useContext, useSyncExternalStore } from "react";

type Theme = "light" | "dark";

const ThemeContext = createContext<{ theme: Theme; toggle: () => void }>({
  theme: "dark",
  toggle: () => {},
});

// documentElement's data-theme attribute is the single source of truth (set
// pre-hydration by the boot script in app/layout.tsx). useSyncExternalStore
// subscribes to it directly instead of mirroring it into useState — no
// setState-in-effect, and it stays correct if anything else ever flips the
// attribute.
function subscribe(onChange: () => void) {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  return () => observer.disconnect();
}

function getSnapshot(): Theme {
  return (document.documentElement.dataset.theme as Theme) || "dark";
}

function getServerSnapshot(): Theme {
  return "dark";
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const theme = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  function toggle() {
    const next: Theme = theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    localStorage.setItem("rosette-theme", next);
  }

  return <ThemeContext.Provider value={{ theme, toggle }}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
  return useContext(ThemeContext).theme;
}

export function useThemeToggle() {
  return useContext(ThemeContext).toggle;
}
