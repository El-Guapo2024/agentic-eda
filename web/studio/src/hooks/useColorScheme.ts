import { useEffect, useState } from "react";
import { useCommonOptions } from "../state/commonOptions";
import { iconSchemeOf } from "../kicad-port/commonSettings";

/** Live-updating "light" | "dark", following the OS (prefers-color-scheme) -- used to pick the matching KiCad icon set (public/icons/{light,dark}/) and to know which chrome palette is active. */
export function useColorScheme(): "light" | "dark" {
  const query = "(prefers-color-scheme: dark)";
  const [scheme, setScheme] = useState<"light" | "dark">(() => (typeof window !== "undefined" && window.matchMedia(query).matches ? "dark" : "light"));

  useEffect(() => {
    const mql = window.matchMedia(query);
    const onChange = () => setScheme(mql.matches ? "dark" : "light");
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, []);

  return scheme;
}

/**
 * The icon set to draw (public/icons/{light,dark}/): Preferences > Common's "Icon theme:" (`COMMON_SETTINGS::m_Appearance.icon_theme`), whose Automatic
 * (the default) follows the system's light or dark mode as `useColorScheme` does (`KIPLATFORM::UI::IsDarkTheme`).
 */
export function useIconScheme(): "light" | "dark" {
  const system = useColorScheme();
  return iconSchemeOf(useCommonOptions().common.iconTheme, system);
}
