import { LitElement, html, css, nothing } from "lit";
import { customElement, property, state } from "lit/decorators.js";

export interface MenuItem {
  id: string;
  label: string;
  /** Optional checkbox state (for collection toggles). */
  checked?: boolean;
  /** Optional submenu (rendered inline when present). */
  submenu?: MenuItem[];
  disabled?: boolean;
}

@customElement("context-menu")
export class ContextMenu extends LitElement {
  static styles = css`
    :host { position: fixed; z-index: 50; }
    .panel {
      min-width: 200px;
      background: var(--surface-elevated);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md, 8px);
      padding: 6px;
      box-shadow: 0 8px 28px rgba(0, 0, 0, 0.4);
    }
    button.item {
      display: flex; align-items: center; gap: 8px;
      width: 100%; text-align: left;
      border: none; background: transparent; cursor: pointer;
      color: var(--on-surface);
      font: 500 13px/1.4 var(--font-body);
      padding: 7px 10px; border-radius: 6px;
    }
    button.item:hover, button.item:focus-visible {
      background: var(--surface); outline: none;
    }
    button.item[disabled] { opacity: 0.4; cursor: default; }
    .check { width: 14px; }
    .sub { margin-left: 8px; border-left: 1px solid var(--hairline); }
  `;

  @property({ attribute: false }) items: MenuItem[] = [];
  @property({ type: Number }) x = 0;
  @property({ type: Number }) y = 0;
  @state() private open = false;

  #onDocClick = (e: MouseEvent) => {
    if (!e.composedPath().includes(this)) this.close();
  };
  #onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") this.close();
  };

  openAt(x: number, y: number, items: MenuItem[]): void {
    this.items = items;
    // Clamp to viewport (assume ~220x320 menu box).
    this.x = Math.min(x, window.innerWidth - 230);
    this.y = Math.min(y, window.innerHeight - 330);
    this.open = true;
    this.style.left = `${this.x}px`;
    this.style.top = `${this.y}px`;
    document.addEventListener("mousedown", this.#onDocClick);
    document.addEventListener("keydown", this.#onKey);
    this.updateComplete.then(() =>
      this.renderRoot.querySelector<HTMLButtonElement>("button.item")?.focus(),
    );
  }
  close(): void {
    this.open = false;
    document.removeEventListener("mousedown", this.#onDocClick);
    document.removeEventListener("keydown", this.#onKey);
  }
  disconnectedCallback(): void {
    super.disconnectedCallback();
    this.close();
  }

  private select(item: MenuItem): void {
    if (item.disabled || item.submenu) return;
    this.dispatchEvent(
      new CustomEvent("menu-select", { detail: item.id, bubbles: true, composed: true }),
    );
    this.close();
  }

  private renderItems(items: MenuItem[]): unknown[] {
    return items.map(
      (it) => html`
        <button class="item" role="menuitem" ?disabled=${it.disabled} @click=${() => this.select(it)}>
          <span class="check" aria-hidden="true">${it.checked ? "✓" : ""}</span>
          <span>${it.label}</span>
        </button>
        ${it.submenu
          ? html`<div class="sub">${this.renderItems(it.submenu)}</div>`
          : nothing}
      `,
    );
  }

  render() {
    if (!this.open) return nothing;
    return html`<div class="panel" role="menu">${this.renderItems(this.items)}</div>`;
  }
}

declare global {
  interface HTMLElementTagNameMap { "context-menu": ContextMenu; }
}
