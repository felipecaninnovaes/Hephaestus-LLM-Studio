import type { Metadata } from "next";
import "./globals.css";

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
    <html lang="pt-BR" className="dark">
      <body>{children}</body>
    </html>
  );
}
