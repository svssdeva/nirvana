import { LitElement, html, css, nothing } from "lit";
import { customElement, property } from "lit/decorators.js";
import type { Game } from "../../ipc";
import "./game-tile";

/** Top-N games that have been played, most-recent first. Pure + testable. */
export function recentlyPlayed(games: Game[], limit = 8): Game[] {
  return games
    .filter((g) => g.lastPlayed != null)
    .sort((a, b) => (b.lastPlayed as number) - (a.lastPlayed as number))
    .slice(0, limit);
}

@customElement("continue-row")
export class ContinueRow extends LitElement {
  static styles = css`
    :host {
      display: block;
    }
    h2 {
      margin: 0 0 12px;
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 20px;
      color: var(--on-surface);
    }
    .row {
      display: grid;
      grid-auto-flow: column;
      grid-auto-columns: 180px;
      gap: 16px;
      overflow-x: auto;
      padding-bottom: 8px;
      scrollbar-width: thin;
    }
  `;

  @property({ attribute: false }) games: Game[] = [];

  render() {
    if (this.games.length === 0) return nothing;
    return html`
      <section aria-label="Continue playing">
        <h2>Continue playing</h2>
        <div class="row">
          ${this.games.map((g) => html`<game-tile .game=${g}></game-tile>`)}
        </div>
      </section>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "continue-row": ContinueRow;
  }
}
