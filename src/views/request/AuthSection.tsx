import type { Auth } from "../../ipc";
import { Field, Select } from "../../ui";
import { AUTH_TYPES, switchAuth, type AuthType, type VarInfo } from "./logic";
import { VarInput } from "./VarInput";

export const LABEL: Record<AuthType, string> = { inherit: "Inherit (folder, collection)", none: "No auth", bearer: "Bearer token", basic: "Basic", apiKey: "API key" };

export interface AuthSectionProps {
  auth: Auth;
  onChange: (auth: Auth) => void;
  info: VarInfo;
  readOnly: boolean;
  /** A collection has nothing to inherit from, so its inherit state reads as "No auth" and the option is not offered. */
  scope?: "request" | "collection";
}

export function AuthSection({ auth, onChange, info, readOnly, scope = "request" }: AuthSectionProps) {
  const collection = scope === "collection";
  const types = collection ? AUTH_TYPES.filter((t) => t !== "inherit") : AUTH_TYPES;
  return (
    <div className="rv-auth">
      <Select aria-label="Auth type" value={collection && auth.type === "inherit" ? "none" : auth.type} disabled={readOnly} onChange={(e) => onChange(switchAuth(auth, e.target.value as AuthType))}>
        {types.map((t) => (
          <option key={t} value={t}>{LABEL[t]}</option>
        ))}
      </Select>

      {auth.type === "bearer" && (
        <Field label="Token">{() => <VarInput value={auth.token} info={info} ariaLabel="Bearer token" placeholder="{{apiToken}}" disabled={readOnly} onChange={(token) => onChange({ ...auth, token })} />}</Field>
      )}

      {auth.type === "basic" && (
        <>
          <Field label="Username">{() => <VarInput value={auth.username} info={info} ariaLabel="Username" placeholder="{{user}}" disabled={readOnly} onChange={(username) => onChange({ ...auth, username })} />}</Field>
          <Field label="Password">{() => <VarInput value={auth.password} info={info} ariaLabel="Password" placeholder="{{password}}" disabled={readOnly} onChange={(password) => onChange({ ...auth, password })} />}</Field>
        </>
      )}

      {auth.type === "apiKey" && (
        <>
          <div className="rv-row">
            <Field label="Name" className="rv-grow">{() => <VarInput value={auth.name} info={info} ariaLabel="API key name" placeholder="X-Api-Key" disabled={readOnly} onChange={(name) => onChange({ ...auth, name })} />}</Field>
            <Field label="Send in">
              {(id) => (
                <Select id={id} value={auth.in} disabled={readOnly} onChange={(e) => onChange({ ...auth, in: e.target.value as "header" | "query" })}>
                  <option value="header">Header</option>
                  <option value="query">Query</option>
                </Select>
              )}
            </Field>
          </div>
          <Field label="Value">{() => <VarInput value={auth.value} info={info} ariaLabel="API key value" placeholder="{{apiKey}}" disabled={readOnly} onChange={(value) => onChange({ ...auth, value })} />}</Field>
        </>
      )}

      {(auth.type === "bearer" || auth.type === "basic" || auth.type === "apiKey") && (
        <p className="rv-hint">Use a <code>{"{{variable}}"}</code> backed by a secret so the value stays in the keyring and out of the request file.</p>
      )}
      {auth.type === "inherit" && !collection && <p className="rv-hint">Uses the auth set on the folder or collection, if any.</p>}
    </div>
  );
}
