import { Clock3, Eye, EyeOff, Link2, LoaderCircle, Ticket } from "lucide-react";
import { useState, type FormEvent } from "react";
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
}

const INVITE_MIN_LENGTH = 32;

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
}: EnrollmentPanelProps) {
  const [mode, setMode] = useState<EnrollmentMode>(initialMode);
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
    await onEnroll({ serverUrl, deviceName, accessCode: code, nodeKey: null });
  };

  return (
    <section className="enrollment-panel" aria-labelledby="enroll-title">
      <div className="section-heading">
        <span className="section-kicker">New connection</span>
        <h2 id="enroll-title">Add a Ket server</h2>
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
                onChange={(event) => setInvite(event.target.value.replace(/\s/g, ""))}
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
        {issue ? (
          <div className="inline-issue" role="alert">
            {issue.message}
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
