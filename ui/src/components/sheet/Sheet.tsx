import { AnimatePresence, motion } from "motion/react";
import { X } from "lucide-react";
import { useEffect, type ReactNode } from "react";
import "./Sheet.css";

/**
 * A window-modal sheet: dims the window and slides the panel down from the
 * top, like `.sheet` on macOS. Esc closes it.
 */
export function Modal({ open, onClose, children }: { open: boolean; onClose: () => void; children: ReactNode }) {
  useEffect(() => {
    if (!open) return;
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [open, onClose]);
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="modal-layer"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
          onPointerDown={(e) => e.target === e.currentTarget && e.stopPropagation()}
        >
          <motion.div
            className="modal-panel"
            initial={{ y: -24, opacity: 0.6 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -24, opacity: 0 }}
            transition={{ type: "spring", stiffness: 420, damping: 38 }}
          >
            {children}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** `FLSheet`: title + close, content, divider, footer. */
export function FLSheet({
  title,
  width = 520,
  height = 500,
  onClose,
  children,
  footer,
}: {
  title: string;
  width?: number;
  height?: number;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <div className="fl-sheet" style={{ width, height }}>
      <div className="fl-sheet__header">
        <span className="fl-sheet__title">{title}</span>
        <button className="fl-sheet__close" onClick={onClose}>
          <X size={15} strokeWidth={2} />
        </button>
      </div>
      <div className="fl-sheet__divider" />
      <div className="fl-sheet__content">{children}</div>
      {footer && (
        <>
          <div className="fl-sheet__divider" />
          <div className="fl-sheet__footer">{footer}</div>
        </>
      )}
    </div>
  );
}
