import { useEffect, useRef, useState } from "react";

export type MenuItem = {
  label?: string;
  onClick?: () => void;
  disabled?: boolean;
  sep?: boolean;
  hint?: string;
};

type MenuState = { x: number; y: number; items: MenuItem[] } | null;

export function useContextMenu() {
  const [menu, setMenu] = useState<MenuState>(null);
  const ref = useRef<HTMLDivElement | null>(null);
  const opener = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      // Let a click on the opener itself reach the opener's click handler,
      // so it can decide whether to toggle the menu shut.
      if (opener.current && opener.current.contains(e.target as Node)) return;
      if (ref.current && ref.current.contains(e.target as Node)) return;
      setMenu(null);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "Tab") {
        e.preventDefault();
        setMenu(null);
        const el = opener.current;
        if (el) (el.closest<HTMLElement>("[tabindex='0'], button, [role='tab']") ?? el).focus();
        return;
      }
      if (!ref.current) return;
      const items = Array.from(
        ref.current.querySelectorAll<HTMLButtonElement>("button.ctx-item:not(:disabled)")
      );
      if (items.length === 0) return;
      const idx = items.indexOf(document.activeElement as HTMLButtonElement);
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const i = idx === -1 ? 0 : idx;
        const n = e.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
        items[n].focus();
      } else if (e.key === "Home") {
        e.preventDefault();
        items[0].focus();
      } else if (e.key === "End") {
        e.preventDefault();
        items[items.length - 1].focus();
      }
    };
    const onResize = () => setMenu(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", key);
    window.addEventListener("resize", onResize);
    const first = ref.current?.querySelector<HTMLButtonElement>("button.ctx-item:not(:disabled)");
    first?.focus();
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key);
      window.removeEventListener("resize", onResize);
    };
  }, [menu]);

  const open = (e: React.MouseEvent, items: MenuItem[], anchor?: { x: number; y: number }) => {
    e.preventDefault();
    e.stopPropagation();
    const target = e.target instanceof HTMLElement ? e.target : null;
    // Re-activating the opener toggles the menu shut (mousedown is suppressed
    // for the opener so this handler is what sees the click).
    if (menu && target && opener.current && opener.current.contains(target)) {
      setMenu(null);
      return;
    }
    opener.current = target;
    const w = 240;
    const h = Math.min(items.length * 26 + 8, 400);
    const rawX = anchor ? anchor.x : e.clientX;
    const rawY = anchor ? anchor.y : e.clientY;
    const x = Math.min(rawX, window.innerWidth - w - 8);
    const y = Math.min(rawY, window.innerHeight - h - 8);
    setMenu({ x: Math.max(4, x), y: Math.max(4, y), items });
  };
  const closeNow = () => {
    setMenu(null);
    const el = opener.current;
    if (el) (el.closest<HTMLElement>("[tabindex='0'], button, [role='tab']") ?? el).focus();
  };
  const node = menu ? (    <div
      ref={ref}
      className="ctx-menu"
      style={{ left: menu.x, top: menu.y }}
      role="menu"
    >
      {menu.items.map((it, i) =>
        it.sep ? (
          <div key={i} className="ctx-sep" role="separator" />
        ) : (
          <button
            key={i}
            role="menuitem"
            tabIndex={-1}
            className="ctx-item"
            disabled={it.disabled}
            onClick={() => {
              if (it.disabled) return;
              setMenu(null);
              it.onClick?.();
            }}
          >
            <span>{it.label}</span>
            {it.hint && <span className="ctx-hint">{it.hint}</span>}
          </button>
        )
      )}
    </div>
  ) : null;

  return { open, close: closeNow, node, isOpen: menu !== null };
}
