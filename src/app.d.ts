// See https://svelte.dev/docs/kit/types#app.d.ts
declare global {
  namespace App {
    // interface Error {}
    // interface Locals {}
    // interface PageData {}
    // interface PageState {}
    // interface Platform {}
  }

  // Injected at build time by Vite (see vite.config.ts) — the git commit the app
  // was built from, or "dev" for local/unstamped builds. Used to detect updates.
  const __APP_COMMIT__: string;
}

export {};
