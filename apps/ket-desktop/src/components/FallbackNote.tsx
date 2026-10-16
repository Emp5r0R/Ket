import type { ClientSnapshot } from "../types";

interface FallbackNoteProps {
  snapshot: ClientSnapshot;
}

/**
 * Says why Ket is not using the protocols it tried first: each transport the
 * latest connection attempt left, with the reason. Nothing when it left none.
 */
export function FallbackNote({ snapshot }: FallbackNoteProps) {
  const { fallbacks, active_transport: active, phase } = snapshot;
  if (fallbacks.length === 0) return null;
  const heading = phase === "connected" && active !== null
    ? <>Using <strong>{active.display_name}</strong> because:</>
    : <>Tried:</>;
  return (
    <div className="fallback-note" role="status" aria-label="Why Ket switched protocols">
      <span>{heading}</span>
      <ul>
        {fallbacks.map((fallback, index) => (
          <li key={`${fallback.transport.id}-${index}`}>
            <strong>{fallback.transport.display_name}</strong>: {fallback.issue.message}
          </li>
        ))}
      </ul>
    </div>
  );
}
