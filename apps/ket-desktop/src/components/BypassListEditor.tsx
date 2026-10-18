import { useState, type FormEvent } from "react";
import { Plus, Route, X } from "lucide-react";
import type { ClientIssue } from "../types";
import { MAX_BYPASS_ENTRIES, bypassRefusal } from "../lib/bypass";

interface BypassListEditorProps {
  entries: string[];
  /** Checks an entry and returns the form it is kept in. */
  check: (entry: string) => Promise<string>;
  onChange: (entries: string[]) => void;
}

/** Settings for addresses and networks reached directly, outside the tunnel. */
export function BypassListEditor({ entries, check, onChange }: BypassListEditorProps) {
  const [draft, setDraft] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const full = entries.length >= MAX_BYPASS_ENTRIES;

  const add = async (event: FormEvent) => {
    event.preventDefault();
    if (draft.trim() === "" || checking) return;
    setChecking(true);
    try {
      const entry = await check(draft);
      onChange(entries.includes(entry) ? entries : [...entries, entry]);
      setDraft("");
      setRefusal(null);
    } catch (error) {
      setRefusal(bypassRefusal((error as Partial<ClientIssue>)?.code ?? ""));
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="settings-bypass">
      <div className="settings-bypass-intro">
        <span className="settings-row-icon"><Route size={18} aria-hidden="true" /></span>
        <span>
          <strong>Keep outside the tunnel</strong>
          <small>
            Addresses and networks listed here are reached directly, with every protocol: a work
            network, or a site that refuses VPN traffic. The network you are on, and anyone watching
            it, can see that traffic and where it goes. Your home network is always reached directly.
          </small>
        </span>
      </div>
      <form className="settings-bypass-add" onSubmit={(event) => void add(event)}>
        <input
          type="text"
          value={draft}
          placeholder="203.0.113.5 or 192.168.10.0/24"
          aria-label="Address or network to keep outside the tunnel"
          aria-invalid={refusal !== null}
          aria-describedby={refusal !== null ? "bypass-refusal" : undefined}
          spellCheck={false}
          autoCapitalize="off"
          disabled={full}
          onChange={(event) => {
            setDraft(event.target.value);
            setRefusal(null);
          }}
        />
        <button type="submit" className="secondary-button" disabled={full || checking || draft.trim() === ""}>
          <Plus size={16} aria-hidden="true" />
          Add
        </button>
      </form>
      {refusal !== null ? <p id="bypass-refusal" className="settings-bypass-refusal" role="alert">{refusal}</p> : null}
      {full ? <p className="settings-bypass-refusal">The list is full ({MAX_BYPASS_ENTRIES} entries).</p> : null}
      {entries.length > 0 ? (
        <ul className="settings-bypass-list" aria-label="Kept outside the tunnel">
          {entries.map((entry) => (
            <li key={entry}>
              <code>{entry}</code>
              <button
                type="button"
                className="icon-button"
                aria-label={`Stop keeping ${entry} outside the tunnel`}
                onClick={() => onChange(entries.filter((kept) => kept !== entry))}
              >
                <X size={15} aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
