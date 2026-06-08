import { LitElement, html, css, nothing } from "lit";
import { customElement, property } from "lit/decorators.js";
import type { Game } from "../../ipc";
import "./game-tile";

/** Top-N games that have been played, most-recent first. Pure + testable. */
export function recentlyPlayed(games: Game[], limit = 5): Game[] {
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
    /* Up to 5 games in a single non-scrolling row, stepping down with width to
       match the main grid's tile size. */
    .row {
      display: grid;
      grid-template-columns: repeat(5, 1fr);
      gap: 16px;
    }
    @media (max-width: 1400px) {
      .row {
        grid-template-columns: repeat(4, 1fr);
      }
    }
    @media (max-width: 1100px) {
      .row {
        grid-template-columns: repeat(3, 1fr);
      }
    }
    @media (max-width: 760px) {
      .row {
        grid-template-columns: repeat(2, 1fr);
      }
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
