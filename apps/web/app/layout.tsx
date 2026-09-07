import type { Metadata } from "next";
import { JetBrains_Mono, Space_Grotesk } from "next/font/google";
import "./globals.css";

const display = Space_Grotesk({
  subsets: ["latin"],
  variable: "--font-display",
  display: "swap",
});

const mono = JetBrains_Mono({
  subsets: ["latin"],
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
