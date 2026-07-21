import { CheckCircle2, CircleOff, Network, ShieldCheck } from "lucide-react";
import { protocolCatalog, protocolInfo, type ProtocolId } from "../lib/protocols";

interface ProtocolsViewProps {
  selected: ProtocolId;
  available: Set<ProtocolId>;
  onSelect: (protocol: ProtocolId) => void;
}

export function ProtocolsView({ selected, available, onSelect }: ProtocolsViewProps) {
  const info = protocolInfo(selected);
  const offered = available.has(selected);

  return (
    <section className="protocols-view" aria-labelledby="protocol-guide-heading">
      <header className="view-header">
        <div>
          <span className="section-kicker">Transport guide</span>
          <h1 id="protocol-guide-heading">{info.label}</h1>
        </div>
        <span className={offered ? "protocol-availability is-offered" : "protocol-availability"}>
          {offered ? <CheckCircle2 size={15} /> : <CircleOff size={15} />}
          {offered ? "Offered by node" : "Not offered by node"}
        </span>
      </header>

      <div className="protocol-guide-layout">
        <nav className="protocol-list" aria-label="Protocol guides">
          {protocolCatalog.map((protocol) => (
            <button
              key={protocol.id}
              type="button"
              className={protocol.id === selected ? "protocol-list-item is-active" : "protocol-list-item"}
              aria-current={protocol.id === selected ? "page" : undefined}
              onClick={() => onSelect(protocol.id)}
            >
              <Network size={17} aria-hidden="true" />
              <span>{protocol.label}</span>
              <i className={available.has(protocol.id) ? "is-offered" : ""} aria-hidden="true" />
            </button>
          ))}
        </nav>

        <article className="protocol-detail">
          <div className="protocol-lead">
            <ShieldCheck size={22} aria-hidden="true" />
            <p>{info.shortInstruction}</p>
          </div>
          <section>
            <h2>Best for</h2>
            <p>{info.bestFor}</p>
          </section>
          <section>
            <h2>How it works</h2>
            <p>{info.operation}</p>
          </section>
          <section>
            <h2>Connect</h2>
            <ol>{info.steps.map((step) => <li key={step}>{step}</li>)}</ol>
          </section>
          <section>
            <h2>Limits</h2>
            <ul>{info.limitations.map((limit) => <li key={limit}>{limit}</li>)}</ul>
          </section>
        </article>
      </div>
    </section>
  );
}
