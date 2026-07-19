"use client";

import { useTheme, useThemeToggle } from "@/components/theme-provider";

export function ThemeToggle({ className }: { className?: string }) {
  const theme = useTheme();
  const toggle = useThemeToggle();

  return (
    <button
      type="button"
      onClick={toggle}
      className={`rounded-full bg-surface px-5 py-2.5 text-sm font-semibold text-ink hover:bg-surface-2 ${className ?? ""}`}
    >
      {theme === "dark" ? "Switch to light" : "Switch to dark"}
    </button>
  );
}
