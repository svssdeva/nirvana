import { LitElement, html, css } from "lit";
import { customElement } from "lit/decorators.js";
import { provide } from "@lit/context";
import { Store, StoreController, storeContext, type View } from "../store";
import { applyTheme, readStoredTheme } from "../theme";
import { dismissSplash } from "../ipc";

/** `document` augmented with the View Transitions API (not in every lib.dom). */
type DocumentWithViewTransitions = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};
import "./app-nav";
import "./library/library-view";
import "./disk/disk-view";
import "./monitor/monitor-view";
import "./settings-view";

/**
 * Root shell: owns the single `Store`, provides it down the tree via
 * `@lit/context`, and renders the primary nav above the active view. Layout is
 * a flat full-bleed dark canvas (design.md §Layout — no chrome between bands).
 * Feature views mount lazily as M1–M4 land; the shell only routes between them.
 */
@customElement("app-root")
export class AppRoot extends LitElement {
  static styles = css`
    :host {
      display: flex;
      flex-direction: column;
      min-height: 100vh;
      background: var(--bg);
      color: var(--on-surface);
    }
    main {
      flex: 1;
    }
    footer {
      padding: 12px 48px;
      border-top: 1px solid var(--hairline);
      font-size: 12px;
      letter-spacing: 0.3px;
      color: var(--on-surface-muted);
      text-align: center;
    }
    footer .brand {
      font-weight: 600;
      color: var(--on-surface);
    }
    @media (max-width: 768px) {
      footer {
        padding: 12px 24px;
      }
    }
  `;

  @provide({ context: storeContext })
  private store = new Store(readStoredTheme());

  constructor() {
    super();
    // The shell owns the store, so it exists here. The controller registers
    // itself with the host (side-effect) and re-renders on view/theme changes.
    new StoreController(this, this.store);
    // Reflect the initial theme to <html> before first paint.
    applyTheme(this.store.theme);
  }

  override firstUpdated(): void {
    // Tauri-side splash: the main window starts hidden behind a splashscreen
    // window. Once the UI has painted, reveal main and close the splash. A short
    // floor keeps the splash visible long enough to register.
    setTimeout(() => void dismissSplash(), 900);
  }

  protected updated() {
    // Re-apply only when the chosen theme diverges from the document — keeps
    // localStorage writes to actual toggles, not every render.
    if (document.documentElement.dataset.theme !== this.store.theme) {
      applyTheme(this.store.theme);
    }
  }

  #lastView: View | null = null;

  /**
   * Wrap view switches in a CSS View Transition so the active view cross-fades
   * (see `::view-transition-*` in styles.css). Only fires on an actual view
   * change — theme toggles keep their own surface-color transition — and only
   * when supported + motion is allowed; otherwise it's a plain synchronous
   * update. Lit's `performUpdate` runs synchronously inside the callback, so the
   * API captures the post-update DOM.
   */
  protected override scheduleUpdate(): void | Promise<unknown> {
    const previous = this.#lastView;
    const next = this.store.view;
    this.#lastView = next;

    const start = (document as DocumentWithViewTransitions).startViewTransition;
    const reduceMotion =
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    const viewChanged = previous !== null && previous !== next;

    if (viewChanged && typeof start === "function" && !reduceMotion) {
      start.call(document, () => {
        super.scheduleUpdate();
      });
      return;
    }
    return super.scheduleUpdate();
  }

  render() {
    return html`
      <app-nav></app-nav>
      <main>${this.renderView()}</main>
      <footer>Powered by <span class="brand">BeyondCodeKarma</span></footer>
    `;
  }

  private renderView() {
    switch (this.store.view) {
      case "library":
        return html`<library-view></library-view>`;
      case "disk":
        return html`<disk-view></disk-view>`;
      case "monitor":
        return html`<monitor-view></monitor-view>`;
      case "settings":
        return html`<settings-view></settings-view>`;
    }
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "app-root": AppRoot;
  }
}
