import { useEffect } from "react";
import { useStore } from "../store";
import AgentSheet from "./AgentSheet";
import EnvironmentsSheet from "./EnvironmentsSheet";
import SettingsSheet from "./SettingsSheet";
import "../styles/tokens-light.css";
import "../styles/sheets.css";

/** Applies the theme to `<html data-theme>`; "system" follows the OS and updates live. */
function useTheme() {
  const theme = useStore((s) => s.settings?.theme);
  useEffect(() => {
    if (!theme) return;
    const root = document.documentElement;
    if (theme !== "system") return void (root.dataset.theme = theme);
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => (root.dataset.theme = mq.matches ? "light" : "dark");
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
}

/** The environments, agent and settings sheets, by `store.sheet`. The collections sheet is rendered elsewhere. */
export default function Sheets() {
  const sheet = useStore((s) => s.sheet);
  useTheme();
  return (
    <>
      {sheet === "environments" && <EnvironmentsSheet />}
      {sheet === "agent" && <AgentSheet />}
      {sheet === "settings" && <SettingsSheet />}
    </>
  );
}
