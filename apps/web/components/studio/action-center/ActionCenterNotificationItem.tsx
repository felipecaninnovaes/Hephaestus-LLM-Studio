"use client";

import { useRouter } from "next/navigation";
import {
  IconAlertTriangle,
  IconCheck,
  IconInfo,
} from "@/components/icons";
import { formatRelativeTime } from "@/lib/format";
import type { SystemNotification } from "./useSystemNotifications";

export interface ActionCenterNotificationItemProps {
  notification: SystemNotification;
  onClose: () => void;
}

export function ActionCenterNotificationItem({
  notification: notif,
  onClose,
}: ActionCenterNotificationItemProps) {
  const router = useRouter();

  const levelConfig = {
    info: {
      border: "border-blue-500/30",
      bg: "bg-blue-500/10",
      text: "text-blue-400",
      icon: IconInfo,
    },
    warning: {
      border: "border-status-alert/30",
      bg: "bg-status-alert/10",
      text: "text-amber-400",
      icon: IconAlertTriangle,
    },
    success: {
      border: "border-status-success/30",
      bg: "bg-status-success/10",
      text: "text-status-success",
      icon: IconCheck,
    },
    error: {
      border: "border-rose-500/30",
      bg: "bg-rose-500/10",
      text: "text-rose-400",
      icon: IconAlertTriangle,
    },
  }[notif.level];

  const IconComp = levelConfig.icon;
  const categoryName = {
    infra: "Infraestrutura",
    orchestrator: "Orquestrador",
    dataset: "Datasets",
    model: "Modelos",
  }[notif.category];

  return (
    <div className="glass-card group relative overflow-hidden rounded-xl border border-white/10 p-3.5 transition-all duration-200 hover:border-brand-500/30 hover:bg-white/[0.03]">
      <div className="flex items-start gap-3">
        <span
          className={`mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-lg border ${levelConfig.border} ${levelConfig.bg} ${levelConfig.text} backdrop-blur-sm`}
        >
          <IconComp className="size-3.5" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-2">
            <h4 className="text-xs font-semibold text-zinc-100 truncate">
              {notif.title}
            </h4>
            <span className="font-mono text-3xs text-zinc-400 shrink-0">
              {formatRelativeTime(notif.timestamp)}
            </span>
          </div>
          <p className="mt-1 text-xs text-zinc-300 leading-relaxed">
            {notif.message}
          </p>
          <div className="mt-2 flex items-center justify-between gap-2 pt-2 border-t border-white/5">
            <span className="rounded border border-white/10 bg-white/[0.03] px-1.5 py-0.5 font-mono text-3xs uppercase tracking-caps text-zinc-400">
              {categoryName}
            </span>
            {notif.actionLabel && notif.actionHref && (
              <button
                type="button"
                onClick={() => {
                  onClose();
                  if (notif.actionHref) router.push(notif.actionHref);
                }}
                className="text-brand-400 hover:text-brand-300 font-mono text-2xs underline underline-offset-2 cursor-pointer"
              >
                {notif.actionLabel} →
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
