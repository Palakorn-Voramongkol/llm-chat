"use client";
import { ThemeProvider } from "next-themes";
import type { ReactNode } from "react";

/** App-wide client providers. next-themes toggles the `dark` class on <html>
 * (matching globals.css `@custom-variant dark (&:is(.dark *))`), persists the
 * choice, and follows the OS theme until the operator picks one. */
export function Providers({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider
      attribute="class"
      defaultTheme="system"
      enableSystem
      disableTransitionOnChange
    >
      {children}
    </ThemeProvider>
  );
}
