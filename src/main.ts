// Entry point: register the root web component. Each view/feature becomes its
// own Lit element under src/components/ as the launcher grows (see docs/PRD).
import "./components/app-root";

// Production hardening: suppress the WebView's developer context menu and the
// devtools/reload hotkeys so a shipped build feels like an app, not a browser.
// (Release builds already omit the Tauri `devtools` feature, so DevTools can't
// open anyway — this removes the leftover affordances.) Kept on in dev.
if (import.meta.env.PROD) {
  window.addEventListener("contextmenu", (e) => e.preventDefault());
  window.addEventListener("keydown", (e) => {
    const devKey =
      e.key === "F12" ||
      e.key === "F5" ||
      ((e.ctrlKey || e.metaKey) && ["r", "R", "u", "U"].includes(e.key)) ||
      ((e.ctrlKey || e.metaKey) && e.shiftKey && ["i", "I", "j", "J", "c", "C"].includes(e.key));
    if (devKey) e.preventDefault();
  });
}
