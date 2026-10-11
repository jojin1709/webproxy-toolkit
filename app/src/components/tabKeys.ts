import type { KeyboardEvent } from "react";

export function tabListKeyDown(
  e: KeyboardEvent<HTMLElement>,
  index: number,
  disabled: boolean[],
  onSelect: (next: number) => void
): void {
  const count = disabled.length;
  const enabled: number[] = [];
  for (let i = 0; i < count; i++) if (!disabled[i]) enabled.push(i);
  if (enabled.length === 0) return;
  let next = -1;
  if (e.key === "ArrowRight") {
    const pos = enabled.indexOf(index);
    next = pos === -1 ? enabled[0] : enabled[(pos + 1) % enabled.length];
  } else if (e.key === "ArrowLeft") {
    const pos = enabled.indexOf(index);
    next = pos === -1 ? enabled[0] : enabled[(pos - 1 + enabled.length) % enabled.length];
  } else if (e.key === "Home") {
    next = enabled[0];
  } else if (e.key === "End") {
    next = enabled[enabled.length - 1];
  } else {
    return;
  }
  e.preventDefault();
  onSelect(next);
  const parent = e.currentTarget.parentElement;
  if (!parent) return;
  const tabs = Array.from(parent.querySelectorAll<HTMLButtonElement>('button[role="tab"]'));
  tabs[next]?.focus();
}
