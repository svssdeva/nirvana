import { LitElement, html, css } from "lit";
import { customElement, property } from "lit/decorators.js";

/**
 * Placeholder body shared by every view until its feature lands. A flat,
 * centered card on the dark canvas (no resting shadow — design.md §Elevation:
 * "cards lift only on press"). `role="status"` so SRs announce the empty state.
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
  `;

  @property() label = "";
  @property() hint = "";
  @property() glyph = "";

  render() {
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
