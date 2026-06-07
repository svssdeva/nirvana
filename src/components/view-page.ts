import { LitElement, html, css } from "lit";
import { customElement, property } from "lit/decorators.js";

/**
 * Full-bleed dark view scaffold: a `hero-band-dark` header (weight-300 display
 * headline + muted tagline, design.md §Components) above slotted content. Every
 * top-level view composes this so headings stay on the same type ladder.
 */
@customElement("view-page")
export class ViewPage extends LitElement {
  static styles = css`
    :host {
      display: block;
    }
    header {
      padding: 48px 48px 24px;
    }
    h1 {
      margin: 0;
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 44px;
      line-height: 1.25;
      letter-spacing: 0.1px;
      color: var(--on-surface);
    }
    p {
      margin: 8px 0 0;
      max-width: 520px;
      font-size: 18px;
      line-height: 1.5;
      letter-spacing: 0.1px;
      color: var(--on-surface-muted);
    }
    @media (max-width: 768px) {
      header {
        padding: 32px 24px 16px;
      }
      h1 {
        font-size: 32px;
      }
    }
  `;

  @property() heading = "";
  @property() tagline = "";

  render() {
    return html`
      <header>
        <h1>${this.heading}</h1>
        ${this.tagline ? html`<p>${this.tagline}</p>` : ""}
      </header>
      <slot></slot>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "view-page": ViewPage;
  }
}
