import type { HopperError } from "../../ipc";
import { Button, Icon } from "../../ui";
import { openSheet } from "../../store";

const names = (e: HopperError): string[] => (Array.isArray(e.details?.names) ? (e.details!.names as unknown[]).map(String) : []);

/** A failed send, explained in the panel. Never a native dialog. */
export function ErrorPanel({ error, onRetry }: { error: HopperError; onRetry: () => void }) {
  const list = names(error);
  const tls = /tls|cert/i.test(error.code) || /certificate|tls/i.test(error.message);
  const timeout = /timeout|timed_out/i.test(error.code);
  const title =
    error.code === "unresolved_variables" ? "Variables without a value"
    : error.code === "secret_missing" ? "A secret has no value"
    : error.code === "not_approved" ? "Not approved"
    : timeout ? "The request timed out"
    : tls ? "TLS handshake failed"
    : "The request failed";
  return (
    <div className="rv-error" role="alert">
      <h3 className="rv-error-title"><Icon name="warning-circle" /> {title}</h3>
      <p className="rv-error-msg">{error.message}</p>
      {list.length > 0 && (
        <ul className="rv-error-names" aria-label="Names">
          {list.map((n) => (
            <li key={n}><code>{error.code === "secret_missing" ? n : `{{${n}}}`}</code></li>
          ))}
        </ul>
      )}
      {error.hint && <p className="rv-error-hint">{error.hint}</p>}
      {timeout && <p className="rv-error-hint">Raise the timeout on the request, or check that the server is reachable.</p>}
      {tls && <p className="rv-error-hint">The certificate could not be verified. For a local or dev server you can turn TLS verification off in that environment; it stays flagged in red.</p>}
      {error.code === "not_approved" && <p className="rv-error-hint">The send needs an approval. Approve it from the Agent sheet or send it yourself.</p>}
      <div className="rv-error-actions">
        {(error.code === "unresolved_variables" || error.code === "secret_missing") && <Button icon="sliders" onClick={() => openSheet("environments")}>Open environments</Button>}
        <Button icon="arrow-clockwise" onClick={onRetry}>Try again</Button>
      </div>
      <p className="rv-error-code">{error.code}</p>
    </div>
  );
}
