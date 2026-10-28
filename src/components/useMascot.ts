import { useEffect, useReducer } from "react";
import { mascotKind, useStore } from "../store";

/** Re-renders once the "agent is working" window of the latest activity has passed. */
export function useMascot() {
  const [, tick] = useReducer((n: number) => n + 1, 0);
  const lastActivity = useStore((s) => s.activity[0]?.at);
  useEffect(() => {
    if (!lastActivity) return;
    const left = 4000 - (Date.now() - Date.parse(lastActivity));
    if (left <= 0) return;
    const t = setTimeout(tick, left + 20);
    return () => clearTimeout(t);
  }, [lastActivity]);
  return useStore((s) => mascotKind(s));
}
