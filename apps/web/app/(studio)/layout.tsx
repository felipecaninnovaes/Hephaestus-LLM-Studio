"use client";

import TabsBar from "@/components/studio/TabsBar";
import { ToastHost } from "@/components/studio/Toast";
import Topbar from "@/components/studio/Topbar";

export default function StudioLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-screen flex flex-col bg-zinc-950 text-zinc-100">
      <Topbar />
      <TabsBar />
      <main className="flex-1 overflow-y-auto">{children}</main>
      <ToastHost />
    </div>
  );
}
