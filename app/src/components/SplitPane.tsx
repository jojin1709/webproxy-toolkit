import { useEffect, useRef, useState } from "react";

type Props = {
  direction?: "row" | "column";
  initial?: number; // percent for first child
  min?: number;
  max?: number;
  children: [React.ReactNode, React.ReactNode];
};

// Draggable splitter, Burp-style pane resizing.
export default function SplitPane({
  direction = "row",
  initial = 50,
  min = 15,
  max = 85,
  children,
}: Props) {
  const [pct, setPct] = useState(initial);
  const wrap = useRef<HTMLDivElement>(null);
  const drag = useRef(false);

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (!drag.current || !wrap.current) return;
      const r = wrap.current.getBoundingClientRect();
      let p =
        direction === "row"
          ? ((e.clientX - r.left) / r.width) * 100
          : ((e.clientY - r.top) / r.height) * 100;
      p = Math.max(min, Math.min(max, p));
      setPct(p);
    };
    const up = () => {
      drag.current = false;
      document.body.classList.remove("dragging");
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [direction, min, max]);

  const first: React.CSSProperties = { flex: `0 0 calc(${pct}% - 3px)` };
  const second: React.CSSProperties = { flex: "1 1 0" };

  return (
    <div ref={wrap} className={`splitpane ${direction === "row" ? "splitpane-row" : "splitpane-col"}`}>
      <div className="splitpane-pane" style={first}>
        {children[0]}
      </div>
      <div
        className="splitpane-handle"
        role="separator"
        aria-orientation={direction === "row" ? "vertical" : "horizontal"}
        aria-valuenow={pct}
        aria-valuemin={min}
        aria-valuemax={max}
        tabIndex={0}
        onPointerDown={(e) => {
          e.preventDefault();
          drag.current = true;
          document.body.classList.add("dragging");
        }}
        onDoubleClick={() => setPct(initial)}
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft" || e.key === "ArrowUp") setPct((p) => Math.max(min, p - 2));
          if (e.key === "ArrowRight" || e.key === "ArrowDown") setPct((p) => Math.min(max, p + 2));
        }}
      />
      <div className="splitpane-pane" style={second}>
        {children[1]}
      </div>
    </div>
  );
}
