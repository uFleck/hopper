import { secretsToFillIn, type MissingSecret } from "./draft";

/** "3 secrets to fill in", each a button that jumps to where the value is set. */
export function MissingSecretsList({ items, onPick }: { items: MissingSecret[]; onPick: (item: MissingSecret) => void }) {
  if (items.length === 0) return null;
  return (
    <section className="sh-missing" aria-label="Missing secrets">
      <h3 className="sh-missing-title"><i className="ph ph-key" aria-hidden="true" /> {secretsToFillIn(items.length)}</h3>
      <p className="sh-help">These come from the shared environment file and have no value on this machine yet.</p>
      <ul className="sh-missing-list">
        {items.map((m) => (
          <li key={`${m.stage}/${m.application}/${m.name}`}>
            <button type="button" className="sh-missing-item" onClick={() => onPick(m)}>
              <span className="sh-dot is-missing" aria-hidden="true" />
              <span className="sh-missing-env">{m.stage}/{m.application}</span>
              <span className="sh-missing-name">{m.name}</span>
              <span className="sh-missing-go">Set value <i className="ph ph-caret-right" aria-hidden="true" /></span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
