import { Clock3, Eye, EyeOff, Link2, LoaderCircle } from "lucide-react";
import { useState, type FormEvent } from "react";
import { formatDuration } from "../lib/format";
import type { ClientIssue, EnrollmentInput } from "../types";

interface EnrollmentPanelProps {
  initialServerUrl: string;
  initialAccessCode: string;
  accessExpiresAtEpochSeconds: number | null;
  initialDeviceName: string;
  busy: boolean;
  issue: ClientIssue | null;
  onEnroll: (input: EnrollmentInput) => Promise<void>;
}

export function EnrollmentPanel({
  initialServerUrl,
  initialAccessCode,
  accessExpiresAtEpochSeconds,
  initialDeviceName,
  busy,
  issue,
  onEnroll,
}: EnrollmentPanelProps) {
  const [serverUrl, setServerUrl] = useState(initialServerUrl);
  const [deviceName, setDeviceName] = useState(initialDeviceName);
  const [accessCode, setAccessCode] = useState(initialAccessCode);
  const [revealCode, setRevealCode] = useState(false);
  const accessRemaining = accessExpiresAtEpochSeconds === null
    ? null
    : Math.max(0, accessExpiresAtEpochSeconds - Math.floor(Date.now() / 1_000));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const code = accessCode;
    setAccessCode("");
    await onEnroll({ serverUrl, deviceName, accessCode: code });
  };

  return (
    <section className="enrollment-panel" aria-labelledby="enroll-title">
      <div className="section-heading">
        <span className="section-kicker">New connection</span>
        <h2 id="enroll-title">Add a Ket server</h2>
      </div>
      <form onSubmit={submit}>
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
        {accessRemaining !== null ? (
          <div className="saved-access-status" role="status">
            <Clock3 size={15} aria-hidden="true" />
            <span>{formatDuration(accessRemaining)} access left</span>
          </div>
        ) : null}
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
        <button type="submit" className="primary-button" disabled={busy || accessCode.length !== 32}>
          {busy ? <LoaderCircle className="spin" size={18} aria-hidden="true" /> : null}
          Add server
        </button>
      </form>
    </section>
  );
}
