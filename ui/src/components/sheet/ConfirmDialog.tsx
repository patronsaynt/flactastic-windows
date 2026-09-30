import { useCallback } from "react";
import { create } from "zustand";
import appIcon from "../../assets/AppIcon.png";
import { Modal } from "./Sheet";
import "./ConfirmDialog.css";

export interface ConfirmButton {
  title: string;
  destructive?: boolean;
  action: () => void;
}

interface Pending {
  title: string;
  message?: string;
  /** Action buttons; a Cancel button is always appended. */
  buttons: ConfirmButton[];
  /** Label of the closing button ("Cancel", or "OK" for a plain alert). */
  cancelTitle?: string;
}

const useConfirm = create<{ pending: Pending | null }>(() => ({ pending: null }));

/**
 * `.confirmationDialog(titleVisibility: .visible)`, which macOS draws as an
 * alert: icon, bold title, message, then stacked buttons with Cancel last.
 */
export function confirmDialog(p: Pending) {
  useConfirm.setState({ pending: p });
}

/** `.alert(title) { OK } message:`, e.g. "Save Failed". */
export function alertDialog(title: string, message?: string) {
  useConfirm.setState({ pending: { title, message, buttons: [], cancelTitle: "OK" } });
}

export function ConfirmHost() {
  const pending = useConfirm((s) => s.pending);
  const close = useCallback(() => useConfirm.setState({ pending: null }), []);
  return (
    <Modal open={pending != null} onClose={close}>
      {pending && (
        <div className="alert" role="alertdialog" aria-label={pending.title}>
          <img className="alert__icon" src={appIcon} alt="" draggable={false} />
          <div className="alert__title">{pending.title}</div>
          {pending.message && <div className="alert__message">{pending.message}</div>}
          <div className="alert__buttons">
            {pending.buttons.map((b, i) => (
              <button
                key={i}
                className={"alert__button" + (i === 0 && !b.destructive ? " is-default" : "") + (b.destructive ? " is-destructive" : "")}
                autoFocus={i === 0}
                onClick={() => {
                  close();
                  b.action();
                }}
              >
                {b.title}
              </button>
            ))}
            <button
              className={"alert__button" + (pending.buttons.length === 0 ? " is-default" : "")}
              autoFocus={pending.buttons.length === 0}
              onClick={close}
            >
              {pending.cancelTitle ?? "Cancel"}
            </button>
          </div>
        </div>
      )}
    </Modal>
  );
}
