import { useState } from "react";
import { AlertTriangle, Copy, Download, X } from "lucide-react";
import type { ClientUpdate } from "../types";
import { describeUpdate } from "../lib/updates";

const DISMISSED_KEY = "ket.update-dismissed";

function readDismissed(): string | null {
  try {
    return window.localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

function writeDismissed(version: string) {
  try {
    window.localStorage.setItem(DISMISSED_KEY, version);
  } catch {
    // Dismissal is a convenience; without storage it lasts for this window.
  }
}

interface UpdateBannerProps {
  update: ClientUpdate | null;
}

/** The server's update advice: a quiet note, a warning, or a required update. */
export function UpdateBanner({ update }: UpdateBannerProps) {
  const [dismissed, setDismissed] = useState<string | null>(readDismissed);
  const [copied, setCopied] = useState<"idle" | "copied" | "failed">("idle");
  const notice = describeUpdate(update);
  if (notice === null) return null;
  if (notice.dismissible && dismissed === notice.latestVersion) return null;

  const copyLink = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(notice.downloadUrl);
      setCopied("copied");
    } catch {
      setCopied("failed");
    }
  };
  const dismiss = () => {
    writeDismissed(notice.latestVersion);
    setDismissed(notice.latestVersion);
  };
  const Icon = notice.tone === "info" ? Download : AlertTriangle;

  return (
    <section
      className={`update-banner is-${notice.tone}`}
      role={notice.tone === "danger" ? "alert" : "status"}
      aria-label="App update"
    >
      <Icon size={18} aria-hidden="true" className="update-icon" />
      <div className="update-copy">
        <strong>{notice.title}</strong>
        {notice.detail ? <span>{notice.detail}</span> : null}
        <span className="update-link">
          Download: <code>{notice.downloadUrl}</code>
        </span>
      </div>
      <div className="update-actions">
        <button type="button" onClick={() => void copyLink()}>
          <Copy size={15} aria-hidden="true" />
          {copied === "copied" ? "Copied" : copied === "failed" ? "Copy failed" : "Copy link"}
        </button>
        {notice.dismissible ? (
          <button type="button" className="icon-button" aria-label="Hide this update" onClick={dismiss}>
            <X size={15} aria-hidden="true" />
          </button>
        ) : null}
      </div>
    </section>
  );
}
