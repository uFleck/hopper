import { Sprite, type SpriteName } from "../components/Sprite";
import { openRequest, useStore } from "../store";
import "../styles/tray.css";

/** The tray is the hopper itself: every request it recently collected drops in as a gem, coloured by its status. */
const gem = (status: number | null): SpriteName => (status == null ? "diamond" : status >= 500 || status === 0 ? "redstone" : status >= 400 ? "gold" : status >= 300 ? "diamond" : "emerald");

export default function Tray() {
  const recent = useStore((s) => s.recent);
  const sent = recent.filter((r) => r.status != null).length;
  const ok = recent.filter((r) => r.status != null && r.status > 0 && r.status < 400).length;
  return (
    <footer className="tray" aria-label="Recent requests">
      <span className="tray-label">
        <Sprite name="hopper" size={16} />
      </span>
      <div className="tray-items">
        {recent.slice(0, 7).map((r) => (
          <button key={r.id} type="button" className="tray-item" onClick={() => void openRequest(r.id)} title={`${r.method} ${r.name}${r.status != null ? ` · ${r.status}` : ""}`} aria-label={`${r.method} ${r.name}${r.status != null ? `, ${r.status}` : ""}`}>
            <Sprite name={gem(r.status)} size={14} />
          </button>
        ))}
      </div>
      <span className="tray-count">{sent === 0 ? "Nothing sent yet" : `${ok} of ${sent} ok`}</span>
    </footer>
  );
}
