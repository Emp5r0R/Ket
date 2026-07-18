import { Activity, Settings, Shield } from "lucide-react";

export type AppView = "connection" | "metrics" | "settings";

interface NavRailProps {
  active: AppView;
  onChange: (view: AppView) => void;
}

const items = [
  { id: "connection" as const, label: "Connection", icon: Shield },
  { id: "metrics" as const, label: "Metrics", icon: Activity },
  { id: "settings" as const, label: "Settings", icon: Settings },
];

export function NavRail({ active, onChange }: NavRailProps) {
  return (
    <nav className="nav-rail" aria-label="Primary navigation">
      <div className="brand-mark">
        <img src="/ket-mark.svg" alt="" />
        <span>Ket</span>
      </div>
      <div className="nav-items">
        {items.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            className={active === id ? "nav-button is-active" : "nav-button"}
            aria-label={label}
            aria-current={active === id ? "page" : undefined}
            title={label}
            onClick={() => onChange(id)}
          >
            <Icon aria-hidden="true" size={20} strokeWidth={1.8} />
            <span>{label}</span>
          </button>
        ))}
      </div>
    </nav>
  );
}

