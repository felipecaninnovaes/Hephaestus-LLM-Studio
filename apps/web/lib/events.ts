"use client";

export const ACTION_CENTER_EVENT = "hephaestus:open-action-center";

export function openActionCenter() {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(ACTION_CENTER_EVENT));
  }
}
