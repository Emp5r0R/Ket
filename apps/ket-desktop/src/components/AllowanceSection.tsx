import { allowanceView } from "../lib/allowance";
import type { AllowanceStatus } from "../types";

interface AllowanceSectionProps {
  allowance: AllowanceStatus;
}

/** How much of the invite's data allowance its devices have used this period. */
export function AllowanceSection({ allowance }: AllowanceSectionProps) {
  const view = allowanceView(allowance);
  return (
    <section className="capacity-section allowance-section" aria-label="Data allowance">
      <div className="capacity-copy">
        <div>
          <span>{view.period}</span>
          <strong>
            {view.used} of {view.limit}
          </strong>
        </div>
        <span>
          {view.usedUp
            ? `Used up. Ket can connect again from ${view.resets}.`
            : `${view.terms}. Starts again ${view.resets}.`}
        </span>
      </div>
      <div className={`capacity-track${view.usedUp ? " allowance-used-up" : ""}`} aria-hidden="true">
        <span style={{ width: `${view.percent}%` }} />
      </div>
    </section>
  );
}
