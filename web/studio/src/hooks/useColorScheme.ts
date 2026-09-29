import { useEffect, useState } from "react";

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
