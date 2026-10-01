import { X } from "lucide-react";
import { create } from "zustand";
import { useSyncSession } from "../../app/sync";
import { PageHeader } from "../../components/PageHeader";
import { Modal } from "../../components/sheet/Sheet";
import { SyncContent } from "./SyncContent";

/** Whether the Sync window (File → Sync… on the Mac) is open. */
export const useSyncWindow = create<{ open: boolean }>(() => ({ open: false }));
export const openSyncWindow = () => useSyncWindow.setState({ open: true });

/**
 * `SyncView`, shown as a sheet over the main window. Networking runs while
 * it (or Settings ▸ Devices) is on screen.
 */
export function SyncWindowHost() {
  const open = useSyncWindow((s) => s.open);
  const close = () => useSyncWindow.setState({ open: false });
  return (
    <Modal open={open} onClose={close}>
      {open && <SyncWindow onClose={close} />}
    </Modal>
  );
}

function SyncWindow({ onClose }: { onClose: () => void }) {
  useSyncSession();
  return (
    <div className="sync-window" style={{ position: "relative" }}>
      <button className="sync-window__close" onClick={onClose} title="Close">
        <X size={15} strokeWidth={2} />
      </button>
      <div className="sync-window__scroll">
        <PageHeader eyebrow="Library" title="Sync" />
        <div className="sync-caption sync-window__intro">
          Copy your library to and from your other devices over Wi-Fi. Nothing leaves your network.
        </div>
        <SyncContent />
      </div>
    </div>
  );
}
