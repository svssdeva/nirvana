import { LitElement, html, css } from "lit";
import { customElement } from "lit/decorators.js";
import { consume } from "@lit/context";
import { Store, storeContext, type View } from "../store";

const ITEMS: ReadonlyArray<{ view: View; label: string }> = [
  { view: "library", label: "Library" },
  { view: "disk", label: "Disk" },
  { view: "monitor", label: "Monitor" },
  { view: "settings", label: "Settings" },
];

/**
 * Primary navigation (design.md §Navigation `primary-nav` + pill chips): a
 * flat band on the active canvas, wordmark left, pill switches + a theme toggle
 * right. Active pill fills with `{colors.primary}` (the "active filter chip"
 * treatment); inactive pills are muted. Colors come from semantic role tokens,
 * so the band follows the dark/light surface mode automatically.
 */
@customElement("app-nav")
export class AppNav extends LitElement {
  static styles = css`
    /* Sticky header: stays pinned to the top while the page scrolls. The host
       (not the inner <nav>) must be sticky so its containing block is the page. */
    :host {
      position: sticky;
      top: 0;
      z-index: 20;
      display: block;
    }
    nav {
      display: flex;
      align-items: center;
      gap: 24px;
      height: 64px;
      padding: 0 48px;
      background: var(--bg);
      border-bottom: 1px solid var(--hairline);
    }
    .wordmark {
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 22px;
      letter-spacing: 0.1px;
      color: var(--on-surface);
      margin-right: auto;
    }
    .alpha {
      margin-left: 8px;
      padding: 2px 8px;
      border-radius: var(--rounded-full);
      background: var(--surface-elevated);
      border: 1px solid var(--hairline);
      font-size: 11px;
      font-weight: 600;
      letter-spacing: 0.5px;
      text-transform: uppercase;
      color: var(--on-surface-muted);
      vertical-align: middle;
    }
    .pills {
      display: flex;
      gap: 8px;
    }
    button {
      border: none;
      border-radius: var(--rounded-full, 9999px);
      padding: 8px 16px;
      font: 700 14px/1.25 var(--font-body);
      letter-spacing: 0.324px;
      background: transparent;
      color: var(--on-surface-muted);
      cursor: pointer;
    }
    button[aria-current="page"] {
      background: var(--primary);
      color: var(--on-primary);
    }
    button:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .toggle {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      border: 1px solid var(--hairline);
      color: var(--on-surface);
    }
    .toggle .glyph {
      font-size: 16px;
      line-height: 1;
    }
    @media (max-width: 768px) {
      nav {
        padding: 0 24px;
        gap: 12px;
      }
      .toggle .label {
        display: none;
      }
    }
  `;

  @consume({ context: storeContext, subscribe: true })
  private store!: Store;

  #unsubscribe?: () => void;

  connectedCallback() {
    super.connectedCallback();
    // `store` is injected by @consume by the time we're connected. Re-render
    // on view/theme changes. Torn down in disconnectedCallback to avoid leaks.
    this.#unsubscribe = this.store.subscribe(() => this.requestUpdate());
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this.#unsubscribe?.();
    this.#unsubscribe = undefined;
  }

  render() {
    const active = this.store.view;
    const isDark = this.store.theme === "dark";
    return html`
      <nav aria-label="Primary">
        <span class="wordmark">Nirvana<span class="alpha">alpha</span></span>
        <div class="pills">
          ${ITEMS.map(
            (item) => html`
              <button
                aria-current=${active === item.view ? "page" : "false"}
                @click=${() => this.store.setView(item.view)}
              >
                ${item.label}
              </button>
            `,
          )}
        </div>
        <button
          class="toggle"
          aria-pressed=${isDark}
          aria-label=${isDark ? "Switch to light mode" : "Switch to dark mode"}
          @click=${() => this.store.toggleTheme()}
        >
          <span class="glyph" aria-hidden="true">${isDark ? "☾" : "☀"}</span>
          <span class="label">${isDark ? "Dark" : "Light"}</span>
        </button>
      </nav>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "app-nav": AppNav;
  }
}
