import type { Metadata } from "next";
import localFont from "next/font/local";
import "./globals.css";

const display = localFont({
  src: [
    { path: "../fonts/space-grotesk-latin-ext.woff2", weight: "300 700", style: "normal" },
    { path: "../fonts/space-grotesk-latin.woff2", weight: "300 700", style: "normal" },
  ],
  variable: "--font-display",
  display: "swap",
});

const mono = localFont({
  src: [
    { path: "../fonts/jetbrains-mono-latin-ext.woff2", weight: "100 800", style: "normal" },
    { path: "../fonts/jetbrains-mono-latin.woff2", weight: "100 800", style: "normal" },
  ],
  variable: "--font-mono",
  display: "swap",
});

export const metadata: Metadata = {
  title: "Hephaestus LLM Studio",
  description: "Studio local de treino e anotação — esqueleto Next.js (Fatia 1.5)",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="pt-BR" className={`dark ${display.variable} ${mono.variable}`}>
      <body className="bg-[var(--bg)] text-[var(--fg)]">{children}</body>
    </html>
  );
}
