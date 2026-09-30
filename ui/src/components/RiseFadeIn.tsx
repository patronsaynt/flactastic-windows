import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { useSetting } from "../app/settings";
import { useUI } from "../app/store";

/**
 * `RiseFadeIn`: opacity 0→1 and a 10pt offset (direction from Settings),
 * ease-out 0.28 s after `min(index·0.015, 0.25)`. With `id`, an item animates
 * once per run (`revealed*IDs`); `enabled=false` skips the initial burst.
 */
export function RiseFadeIn({
  index = 0,
  delay: fixedDelay,
  id,
  enabled = true,
  children,
  className,
  style,
  ...rest
}: {
  index?: number;
  /** `riseFadeIn(delay:)`: an explicit delay instead of the index stagger. */
  delay?: number;
  id?: string;
  enabled?: boolean;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
} & React.HTMLAttributes<HTMLDivElement>) {
  const on = useSetting("flactastic.fadeAnimationsEnabled");
  const dir = useSetting("flactastic.fadeAnimationDirection");
  const already = id != null && useUI.getState().revealed.has(id);
  const skip = !on || !enabled || already;
  const [shown, setShown] = useState(skip);

  useEffect(() => {
    if (id != null) useUI.getState().revealed.add(id);
    if (shown) return;
    const raf = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(raf);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const delay = fixedDelay ?? Math.min(index * 0.015, 0.25);
  const off = dir === "leftToRight" ? "translateX(-10px)" : dir === "rightToLeft" ? "translateX(10px)" : "translateY(10px)";
  return (
    <div
      className={className}
      {...rest}
      style={{
        ...style,
        opacity: shown ? 1 : 0,
        transform: shown ? "none" : off,
        transition: skip ? undefined : `opacity 0.28s ease-out ${delay}s, transform 0.28s ease-out ${delay}s`,
      }}
    >
      {children}
    </div>
  );
}
