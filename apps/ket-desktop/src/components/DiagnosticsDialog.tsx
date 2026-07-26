import { useEffect, useState } from "react";
import { Check, ClipboardCopy, X } from "lucide-react";

interface DiagnosticsDialogProps {
  open: boolean;
  report: string;
  onClose: () => void;
}

export function DiagnosticsDialog({ open, report, onClose }: DiagnosticsDialogProps) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!open) setCopied(false);
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
    await navigator.clipboard.writeText(report);
    setCopied(true);
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
        <p>
          This report contains connection stages and cleanup results. Access codes and session credentials are excluded.
        </p>
        <pre>{report}</pre>
        <footer>
          <button type="button" className="primary-button" onClick={() => void copy()}>
            {copied ? <Check size={16} /> : <ClipboardCopy size={16} />}
            {copied ? "Copied" : "Copy logs"}
          </button>
        </footer>
      </section>
    </div>
  );
}
