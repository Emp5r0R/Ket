import { Clock3, Eye, EyeOff, ImageUp, Link2, LoaderCircle, Ticket } from "lucide-react";
import { useRef, useState, type ClipboardEvent, type DragEvent, type FormEvent } from "react";
import { formatDuration } from "../lib/format";
import type { ClientIssue, EnrollmentInput } from "../types";

export type EnrollmentMode = "invite" | "manual";

interface EnrollmentPanelProps {
  initialServerUrl: string;
  initialAccessCode: string;
  accessExpiresAtEpochSeconds: number | null;
  initialDeviceName: string;
  /** Invite entry is the default; saved URL + code profiles start in manual mode. */
  initialMode?: EnrollmentMode;
  busy: boolean;
  issue: ClientIssue | null;
  onEnroll: (input: EnrollmentInput) => Promise<void>;
  onEnrollInvite: (invite: string, deviceName: string) => Promise<void>;
  onEnrollInviteImage: (image: Uint8Array, deviceName: string) => Promise<void>;
}

const INVITE_MIN_LENGTH = 32;
const MAX_QR_IMAGE_BYTES = 8 * 1024 * 1024;
const QR_IMAGE_TYPES = ["image/png", "image/jpeg"];

function readFileBytes(file: File): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error);
    reader.readAsArrayBuffer(file);
  });
}

export function EnrollmentPanel({
  initialServerUrl,
  initialAccessCode,
  accessExpiresAtEpochSeconds,
  initialDeviceName,
  initialMode = "invite",
  busy,
  issue,
  onEnroll,
  onEnrollInvite,
  onEnrollInviteImage,
}: EnrollmentPanelProps) {
  const [mode, setMode] = useState<EnrollmentMode>(initialMode);
  const [imageIssue, setImageIssue] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const imageInput = useRef<HTMLInputElement>(null);
  const [invite, setInvite] = useState("");
  const [revealInvite, setRevealInvite] = useState(false);
  const [serverUrl, setServerUrl] = useState(initialServerUrl);
  const [deviceName, setDeviceName] = useState(initialDeviceName);
  const [accessCode, setAccessCode] = useState(initialAccessCode);
  const [revealCode, setRevealCode] = useState(false);
  const accessRemaining = accessExpiresAtEpochSeconds === null
    ? null
    : Math.max(0, accessExpiresAtEpochSeconds - Math.floor(Date.now() / 1_000));
  const canSubmit = mode === "invite"
    ? invite.length >= INVITE_MIN_LENGTH
    : accessCode.length === 32;

  /** Hands a local PNG/JPEG to native QR decoding; nothing is uploaded or kept. */
  const useQrImage = async (file: File) => {
    if (!QR_IMAGE_TYPES.includes(file.type) || file.size === 0 || file.size > MAX_QR_IMAGE_BYTES) {
      setImageIssue("Choose a PNG or JPEG image of a Ket invite QR code (up to 8 MiB).");
      return;
    }
    setImageIssue(null);
    setInvite("");
    let bytes: Uint8Array;
    try {
      bytes = await readFileBytes(file);
    } catch {
      setImageIssue("That image could not be read.");
      return;
    }
    await onEnrollInviteImage(bytes, deviceName);
  };

  const pasteInvite = (event: ClipboardEvent<HTMLInputElement>) => {
    const image = Array.from(event.clipboardData.files).find((file) => file.type.startsWith("image/"));
    if (image) {
      event.preventDefault();
      void useQrImage(image);
    }
  };

  /** A QR image dropped anywhere on the panel is read like an opened one. */
  const dropImage = (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDragging(false);
    if (busy || mode !== "invite") return;
    const image = Array.from(event.dataTransfer.files).find((file) => file.type.startsWith("image/"));
    if (image) void useQrImage(image);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (mode === "invite") {
      const text = invite;
      setInvite("");
      await onEnrollInvite(text, deviceName);
      return;
    }
    const code = accessCode;
    setAccessCode("");
    await onEnroll({ serverUrl, deviceName, accessCode: code, nodeKey: null, sealedControl: false });
  };

  return (
    <section
      className={`enrollment-panel ${dragging ? "is-dropping" : ""}`}
      aria-labelledby="enroll-title"
      onDragOver={(event) => {
        if (mode !== "invite" || busy) return;
        event.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDragging(false);
      }}
      onDrop={dropImage}
    >
      <div className="section-heading">
        <span className="section-kicker">New connection</span>
        <h2 id="enroll-title">Add a Ket server</h2>
        <p className="enrollment-hint">
          {mode === "invite"
            ? "Paste the invite your server's owner sent you, or open or drop its QR code image."
            : "Use the server address and the 32-character access code from your server's owner."}
        </p>
      </div>
      <form onSubmit={submit}>
        {accessRemaining !== null ? (
          <div className="saved-access-status" role="status">
            <Clock3 size={15} aria-hidden="true" />
            <span>{formatDuration(accessRemaining)} access left</span>
          </div>
        ) : null}
        {mode === "invite" ? (
          <div className="field">
            <label htmlFor="invite">Invite</label>
            <span className="input-shell access-code-shell">
              <Ticket size={17} aria-hidden="true" />
              <input
                id="invite"
                name="invite"
                type={revealInvite ? "text" : "password"}
                autoComplete="off"
                autoCapitalize="none"
                spellCheck={false}
                placeholder="ket:invite:..."
                maxLength={2_048}
                value={invite}
                onChange={(event) => {
                  setImageIssue(null);
                  setInvite(event.target.value.replace(/\s/g, ""));
                }}
                onPaste={pasteInvite}
                required
                disabled={busy}
              />
              <button
                type="button"
                className="icon-button input-action"
                onClick={() => setRevealInvite((value) => !value)}
                aria-label={revealInvite ? "Hide invite" : "Show invite"}
                title={revealInvite ? "Hide invite" : "Show invite"}
              >
                {revealInvite ? <EyeOff size={17} /> : <Eye size={17} />}
              </button>
            </span>
            <button
              type="button"
              className="secondary-button"
              onClick={() => imageInput.current?.click()}
              disabled={busy}
            >
              <ImageUp size={16} aria-hidden="true" />
              Open QR image
            </button>
            <input
              ref={imageInput}
              type="file"
              accept={QR_IMAGE_TYPES.join(",")}
              aria-label="QR image file"
              hidden
              onChange={(event) => {
                const file = event.target.files?.[0];
                event.target.value = "";
                if (file) void useQrImage(file);
              }}
            />
          </div>
        ) : (
          <>
            <div className="field">
              <label htmlFor="server-url">Server URL</label>
              <span className="input-shell">
                <Link2 size={17} aria-hidden="true" />
                <input
                  id="server-url"
                  name="server-url"
                  type="url"
                  inputMode="url"
                  autoComplete="url"
                  placeholder="https://node.example.com"
                  value={serverUrl}
                  onChange={(event) => setServerUrl(event.target.value)}
                  required
                  disabled={busy}
                />
              </span>
            </div>
            <div className="field">
              <label htmlFor="access-code">Access code</label>
              <span className="input-shell access-code-shell">
                <input
                  id="access-code"
                  name="access-code"
                  type={revealCode ? "text" : "password"}
                  autoComplete="off"
                  autoCapitalize="none"
                  spellCheck={false}
                  minLength={32}
                  maxLength={32}
                  value={accessCode}
                  onChange={(event) => setAccessCode(event.target.value.replace(/\s/g, ""))}
                  required
                  disabled={busy}
                />
                <span className="field-count">{accessCode.length}/32</span>
                <button
                  type="button"
                  className="icon-button input-action"
                  onClick={() => setRevealCode((value) => !value)}
                  aria-label={revealCode ? "Hide access code" : "Show access code"}
                  title={revealCode ? "Hide access code" : "Show access code"}
                >
                  {revealCode ? <EyeOff size={17} /> : <Eye size={17} />}
                </button>
              </span>
            </div>
          </>
        )}
        <div className="field">
          <label htmlFor="device-name">Device name</label>
          <span className="input-shell">
            <input
              id="device-name"
              name="device-name"
              type="text"
              autoComplete="off"
              maxLength={96}
              value={deviceName}
              onChange={(event) => setDeviceName(event.target.value)}
              required
              disabled={busy}
            />
          </span>
        </div>
        {imageIssue ?? issue ? (
          <div className="inline-issue" role="alert">
            {imageIssue ?? issue?.message}
          </div>
        ) : null}
        <button type="submit" className="primary-button" disabled={busy || !canSubmit}>
          {busy ? <LoaderCircle className="spin" size={18} aria-hidden="true" /> : null}
          Add server
        </button>
        <button
          type="button"
          className="text-button"
          onClick={() => setMode((current) => (current === "invite" ? "manual" : "invite"))}
          disabled={busy}
        >
          {mode === "invite"
            ? "Advanced: use a server URL and access code"
            : "Use an invite instead"}
        </button>
      </form>
    </section>
  );
}
