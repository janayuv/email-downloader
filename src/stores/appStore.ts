import { create } from "zustand";

interface AppStore {
  theme: "light" | "dark" | "system";
  setTheme: (t: "light" | "dark" | "system") => void;
  applyTheme: () => void;
}

function resolve(theme: "light" | "dark" | "system"): boolean {
  if (theme === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  }
  return theme === "dark";
}

export const useAppStore = create<AppStore>((set, get) => ({
  theme: (localStorage.getItem("theme") as AppStore["theme"]) || "system",
  setTheme: (theme) => {
    localStorage.setItem("theme", theme);
    set({ theme });
    get().applyTheme();
  },
  applyTheme: () => {
    const dark = resolve(get().theme);
    document.documentElement.classList.toggle("dark", dark);
  },
}));
