import { LitElement, html, css } from "lit";
import { customElement, state } from "lit/decorators.js";
import { consume } from "@lit/context";
import { Store, storeContext, type View } from "../store";
import { win } from "../ipc";

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
      height: 56px;
      /* No right padding — the window controls sit flush in the corner. */
      padding: 0 0 0 48px;
      background: var(--bg);
      border-bottom: 1px solid var(--hairline);
      /* It doubles as the OS title bar (decorations off) — don't select its text. */
      user-select: none;
      -webkit-user-select: none;
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
    /* Custom window controls (title bar). Flush to the top-right corner. */
    .winctls {
      display: flex;
      align-self: stretch;
      margin-left: 8px;
    }
    .winctl {
      width: 46px;
      height: 100%;
      display: grid;
      place-items: center;
      border: none;
      border-radius: 0;
      padding: 0;
      background: transparent;
      color: var(--on-surface-muted);
      cursor: default;
    }
    .winctl svg {
      width: 11px;
      height: 11px;
    }
    .winctl:hover {
      background: var(--surface-elevated);
      color: var(--on-surface);
    }
    .winctl.close:hover {
      background: #e81123;
      color: #fff;
    }
    .winctl:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: -2px;
    }
    @media (max-width: 768px) {
      nav {
        padding-left: 24px;
        gap: 12px;
      }
      .toggle .label {
        display: none;
      }
    }
  `;

  @consume({ context: storeContext, subscribe: true })
  private store!: Store;

  /** Whether the window is maximized (drives the restore/maximize glyph). */
  @state() private maximized = false;

  #unsubscribe?: () => void;
  #unlistenResized?: () => void;

  connectedCallback() {
    super.connectedCallback();
    // `store` is injected by @consume by the time we're connected. Re-render
    // on view/theme changes. Torn down in disconnectedCallback to avoid leaks.
    this.#unsubscribe = this.store.subscribe(() => this.requestUpdate());
    void this.trackMaximize();
  }

  disconnectedCallback() {
    super.disconnectedCallback();
    this.#unsubscribe?.();
    this.#unsubscribe = undefined;
    this.#unlistenResized?.();
    this.#unlistenResized = undefined;
  }

  /** Keep the maximize/restore glyph in sync with the window state. */
  private async trackMaximize(): Promise<void> {
    try {
      this.maximized = await win.isMaximized();
      const unlisten = await win.onResized(async () => {
        this.maximized = await win.isMaximized();
      });
      if (this.isConnected) this.#unlistenResized = unlisten;
      else unlisten();
    } catch {
      // not in a Tauri window context — controls simply won't reflect state
    }
  }

  /** Drag the window from empty title-bar areas (ignore clicks on controls). */
  private onDragStart(e: PointerEvent): void {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    void win.startDragging().catch(() => {});
  }

  private onTitleDblClick(e: MouseEvent): void {
    if ((e.target as HTMLElement).closest("button")) return;
    void win.toggleMaximize().catch(() => {});
  }

  render() {
    const active = this.store.view;
    const isDark = this.store.theme === "dark";
    return html`
      <nav
        aria-label="Primary"
        @pointerdown=${this.onDragStart}
        @dblclick=${this.onTitleDblClick}
      >
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
        <div class="winctls">
          <button class="winctl" aria-label="Minimize" title="Minimize" @click=${() => win.minimize()}>
            <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5 h10" stroke="currentColor" stroke-width="1" /></svg>
          </button>
          <button
            class="winctl"
            aria-label=${this.maximized ? "Restore" : "Maximize"}
            title=${this.maximized ? "Restore" : "Maximize"}
            @click=${() => win.toggleMaximize()}
          >
            ${this.maximized
              ? html`<svg viewBox="0 0 10 10" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1">
                  <rect x="0.5" y="2.5" width="6" height="6" /><path d="M2.5 2.5 V0.5 H8.5 V6.5 H6.5" />
                </svg>`
              : html`<svg viewBox="0 0 10 10" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1">
                  <rect x="0.5" y="0.5" width="9" height="9" />
                </svg>`}
          </button>
          <button class="winctl close" aria-label="Close" title="Close" @click=${() => win.close()}>
            <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0 L10 10 M10 0 L0 10" stroke="currentColor" stroke-width="1" /></svg>
          </button>
        </div>
      </nav>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "app-nav": AppNav;
  }
}
