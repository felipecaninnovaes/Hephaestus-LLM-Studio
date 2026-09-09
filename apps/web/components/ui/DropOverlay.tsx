"use client";

import React, { useRef, useState, useCallback, type ReactNode } from "react";
import { IconUpload } from "@/components/icons";

export interface UseFileDropOptions {
  onDropFiles: (files: FileList, dataTransfer: DataTransfer) => void | Promise<void>;
  disabled?: boolean;
}

export function useFileDrop({ onDropFiles, disabled = false }: UseFileDropOptions) {
  const [isDragging, setIsDragging] = useState(false);
  const dragCounterRef = useRef(0);

  const handleDragEnter = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      if (disabled) return;
      dragCounterRef.current += 1;
      if (e.dataTransfer.items && e.dataTransfer.items.length > 0) {
        setIsDragging(true);
      }
    },
    [disabled],
  );

  const handleDragLeave = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      if (disabled) return;
      dragCounterRef.current -= 1;
      if (dragCounterRef.current <= 0) {
        setIsDragging(false);
        dragCounterRef.current = 0;
      }
    },
    [disabled],
  );

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);
      dragCounterRef.current = 0;
      if (disabled) return;
      if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
        void onDropFiles(e.dataTransfer.files, e.dataTransfer);
      }
    },
    [disabled, onDropFiles],
  );

  return {
    isDragging,
    dropProps: {
      onDragEnter: handleDragEnter,
      onDragLeave: handleDragLeave,
      onDragOver: handleDragOver,
      onDrop: handleDrop,
    },
  };
}

export interface DropOverlayProps {
  open: boolean;
  title: string;
  subtitle?: string;
  icon?: ReactNode;
  onDragLeave?: (e: React.DragEvent) => void;
  onDrop?: (e: React.DragEvent) => void;
  className?: string;
}

export function DropOverlay({
  open,
  title,
  subtitle,
  icon,
  onDragLeave,
  onDrop,
  className = "",
}: DropOverlayProps) {
  if (!open) return null;

  return (
    <div
      role="region"
      aria-label="Área de soltura de arquivos"
      className={`fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-6 backdrop-blur-md transition-all animate-in fade-in ${className}`}
      onDragOver={(e) => e.preventDefault()}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
    >
      <div className="pointer-events-none flex flex-col items-center gap-4 rounded-3xl border-2 border-dashed border-brand-500/80 bg-brand-500/10 backdrop-blur-sm p-12 text-center shadow-[0_0_60px_rgba(131,80,242,0.3)]">
        <div className="flex h-16 w-16 items-center justify-center rounded-2xl border border-brand-500/40 bg-brand-500/20 backdrop-blur-sm text-brand-300">
          {icon || <IconUpload className="h-8 w-8" />}
        </div>
        <div>
          <p className="font-display text-lg font-bold text-white">
            {title}
          </p>
          {subtitle && (
            <p className="font-mono text-xs text-brand-200/80 mt-1">
              {subtitle}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

export default DropOverlay;
