import { ShieldCheck } from "lucide-react";
import { useDownloads } from "../../app/downloads";
import { Modal } from "../../components/sheet/Sheet";
import { PillButton } from "../../components/settings/Primitives";
import { api } from "../../lib/api";

/**
 * `LucidaChallengeSheet`: while Cloudflare wants a real click, the Lucida
 * webview opens as its own window over the app and this sheet explains it.
 * Both close by themselves once the bridge is ready.
 */
export function LucidaChallengeHost() {
  const open = useDownloads((s) => s.lucida.needsUserChallenge);
  return (
    <Modal open={open} onClose={() => void api.lucidaDismissChallenge()}>
      <div className="dl-challenge">
        <div className="dl-challenge__head">
          <ShieldCheck size={22} className="dl-challenge__icon" />
          <div>
            <div className="dl-challenge__title">Verify with Lucida</div>
            <div className="dl-challenge__body">
              Cloudflare needs a quick human check before downloads can start. Click the checkbox in the verification
              window — it will close automatically.
            </div>
          </div>
        </div>
        <div className="dl-challenge__actions">
          <PillButton muted onClick={() => void api.lucidaDismissChallenge()}>
            Not now
          </PillButton>
          <PillButton onClick={() => void api.lucidaReload()}>Reload</PillButton>
          <PillButton primary onClick={() => void api.lucidaRevealChallenge()}>
            Show window
          </PillButton>
        </div>
      </div>
    </Modal>
  );
}
