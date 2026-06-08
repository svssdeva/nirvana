import { LitElement, html, css } from "lit";
import { customElement, property } from "lit/decorators.js";

/**
 * Placeholder body shared by every view until its feature lands. A flat,
 * centered card on the dark canvas (no resting shadow — design.md §Elevation:
 * "cards lift only on press"). `role="status"` so SRs announce the empty state.
 *
 * `variant="onboarding"` shows a welcome card with "Scan now", "Add watch
 * folders", and "Skip for now" actions that bubble `scan-now`, `add-folders`,
 * and `dismiss` CustomEvents. All other variants render the existing label/hint/
 * glyph content unchanged.
 */
@customElement("empty-state")
export class EmptyState extends LitElement {
  static styles = css`
    :host {
      display: grid;
      place-items: center;
      min-height: 40vh;
      padding: 48px 32px;
    }
    .card {
      max-width: 520px;
      text-align: center;
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md, 8px);
      padding: 48px 32px;
    }
    .glyph {
      font-size: 35px;
      line-height: 1;
      margin-bottom: 16px;
    }
    h2 {
      margin: 0 0 8px;
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 22px;
      line-height: 1.25;
      letter-spacing: 0.1px;
      color: var(--on-surface);
    }
    p {
      margin: 0;
      font-size: 16px;
      line-height: 1.5;
      color: var(--on-surface-muted);
    }
    /* Onboarding variant — action row below the description text. */
    .actions {
      display: flex;
      flex-wrap: wrap;
      justify-content: center;
      gap: 12px;
      margin-top: 28px;
    }
    /* Primary pill — design.md button-primary: --primary bg, --on-primary text,
       button-lg type (18px/700/1.25/0.45px), 12px 28px padding, ~48px height,
       rounded-full. */
    .pill.primary {
      font-family: var(--font-body);
      font-size: 18px;
      font-weight: 700;
      letter-spacing: 0.45px;
      line-height: 1.25;
      color: var(--on-primary);
      background: var(--primary);
      border: none;
      border-radius: var(--rounded-full, 9999px);
      padding: 12px 28px;
      min-height: 48px;
      cursor: pointer;
    }
    .pill.primary:active {
      background: var(--primary-pressed);
    }
    .pill.primary:focus-visible {
      outline: 2px solid var(--on-primary);
      outline-offset: 2px;
    }
    /* Secondary pill — design.md button-secondary-dark: outline on dark canvas. */
    .pill {
      font-family: var(--font-body);
      font-size: 18px;
      font-weight: 700;
      letter-spacing: 0.45px;
      line-height: 1.25;
      color: var(--on-surface);
      background: transparent;
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-full, 9999px);
      padding: 12px 28px;
      min-height: 48px;
      cursor: pointer;
    }
    .pill:not(.primary):active {
      background: var(--surface-elevated);
    }
    .pill:not(.primary):focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    /* Inline text link — no border, muted color. */
    .link {
      background: none;
      border: none;
      padding: 0;
      font-size: 16px;
      color: var(--on-surface-muted);
      cursor: pointer;
      text-decoration: underline;
      text-underline-offset: 3px;
      align-self: center;
    }
    .link:hover {
      color: var(--on-surface);
    }
    .link:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
      border-radius: 2px;
    }
  `;

  @property() variant: "filtered" | "onboarding" = "filtered";
  @property() label = "";
  @property() hint = "";
  @property() glyph = "";

  private emit(name: string): void {
    this.dispatchEvent(new CustomEvent(name, { bubbles: true, composed: true }));
  }

  render() {
    if (this.variant === "onboarding") {
      return html`
        <div class="card" role="status">
          <h2>Welcome to Nirvana</h2>
          <p>
            Nirvana finds your installed Steam, Epic, and GOG games automatically.
          </p>
          <div class="actions">
            <button class="pill primary" @click=${() => this.emit("scan-now")}>
              Scan now
            </button>
            <button class="pill" @click=${() => this.emit("add-folders")}>
              Add watch folders
            </button>
            <button class="link" @click=${() => this.emit("dismiss")}>
              Skip for now
            </button>
          </div>
        </div>
      `;
    }
    return html`
      <div class="card" role="status">
        ${this.glyph ? html`<div class="glyph" aria-hidden="true">${this.glyph}</div>` : ""}
        <h2>${this.label}</h2>
        ${this.hint ? html`<p>${this.hint}</p>` : ""}
      </div>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "empty-state": EmptyState;
  }
}
