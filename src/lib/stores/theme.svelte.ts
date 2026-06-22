import { writable } from "svelte/store";

export const darkMode = writable<boolean>(false);
if (typeof localStorage !== "undefined") {
  const saved = localStorage.getItem("family-finance-dark-mode");
  if (saved !== null) {
    darkMode.set(saved === "true");
  }
  darkMode.subscribe((val) => {
    localStorage.setItem("family-finance-dark-mode", String(val));
  });
}