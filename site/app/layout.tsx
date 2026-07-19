import type { Metadata } from "next";
import { IBM_Plex_Sans, IBM_Plex_Mono } from "next/font/google";
import { ThemeProvider } from "@/components/theme-provider";
import "./globals.css";

const plexSans = IBM_Plex_Sans({
  subsets: ["latin"],
  weight: ["400", "500", "600", "700"],
  variable: "--font-plex-sans",
  display: "swap",
});

const plexMono = IBM_Plex_Mono({
  subsets: ["latin"],
  weight: ["400", "500"],
  variable: "--font-plex-mono",
  display: "swap",
});

export const metadata: Metadata = {
  title: "Rosette — a private messenger for the Chat Control era",
  description:
    "Rosette is a private messenger end-to-end encrypted with MLS. Relays store-and-forward padded ciphertext and can't read names, content, or membership. Free forever for individuals. Open source, AGPL-3.0.",
  metadataBase: new URL("https://rosette.chat"),
  openGraph: {
    title: "Rosette — a private messenger for the Chat Control era",
    description:
      "End-to-end encrypted with MLS. Free forever for individuals — no paid tier, ever.",
    url: "https://rosette.chat",
    siteName: "Rosette",
    type: "website",
  },
};

// Runs before first paint so the manual toggle's stored preference (or the
// OS preference on first visit) applies with no flash of the wrong theme.
const THEME_BOOT_SCRIPT = `
(function () {
  try {
    var stored = localStorage.getItem('rosette-theme');
    var theme = stored || (window.matchMedia && matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark');
    document.documentElement.dataset.theme = theme;
  } catch (e) {}
})();
`;

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className={`${plexSans.variable} ${plexMono.variable}`} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_BOOT_SCRIPT }} />
      </head>
      <body className="font-sans antialiased" suppressHydrationWarning>
        <ThemeProvider>{children}</ThemeProvider>
      </body>
    </html>
  );
}
