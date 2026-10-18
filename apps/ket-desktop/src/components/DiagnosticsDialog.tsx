import { useEffect, useState } from "react";
import { Check, ClipboardCopy, X } from "lucide-react";

interface DiagnosticsDialogProps {
  open: boolean;
  report: string;
  onClose: () => void;
}

export function DiagnosticsDialog({ open, report, onClose }: DiagnosticsDialogProps) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    if (!open) setCopyState("idle");
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose, open]);

  if (!open) return null;

  const copy = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(report);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  };

  return (
    <div className="diagnostics-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="diagnostics-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="diagnostics-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header>
          <div>
            <span className="section-kicker">Troubleshooting</span>
            <h2 id="diagnostics-title">Connection diagnostics</h2>
          </div>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Close diagnostics">
            <X size={18} />
          </button>
        </header>
        <div className="diagnostics-body">
          <p>
            Send this to the person who runs your server. It shows what each protocol tried and why it stopped,
            without addresses, access codes, or the sites you visited.
          </p>
          <pre role="region" tabIndex={0} aria-label="Connection diagnostic report">{report}</pre>
        </div>
        <footer>
          <button type="button" className="primary-button" onClick={() => void copy()}>
            {copyState === "copied" ? <Check size={16} /> : <ClipboardCopy size={16} />}
            {copyState === "copied" ? "Copied" : copyState === "failed" ? "Copy failed" : "Copy logs"}
          </button>
        </footer>
      </section>
    </div>
  );
}
