import { Share2, Square } from "lucide-react";
import { devicesText, type Sharing } from "../lib/sharing";

interface ShareBannerProps {
  sharing: Sharing;
  onOpenDetails: () => void;
}

/** Shown on the main view while this computer shares its connection. */
export function ShareBanner({ sharing, onOpenDetails }: ShareBannerProps) {
  const { view, status, busy, stop } = sharing;
  if (view === null) return null;
  const paused = status !== null && !status.tunnel;
  return (
    <div className={`update-banner share-banner ${paused ? "is-warning" : "is-info"}`} role="status">
      <Share2 className="update-icon" size={18} aria-hidden="true" />
      <div className="update-copy">
        <strong>
          Sharing on {view.details.network.address}:{view.details.port}
        </strong>
        <span>
          {devicesText(status?.devices ?? 0)}
          {paused ? ". Paused while Ket reconnects." : "."}
        </span>
      </div>
      <div className="update-actions">
        <button type="button" onClick={onOpenDetails}>
          Details
        </button>
        <button type="button" disabled={busy} onClick={() => void stop()}>
          <Square size={14} aria-hidden="true" />
          Stop
        </button>
      </div>
    </div>
  );
}
