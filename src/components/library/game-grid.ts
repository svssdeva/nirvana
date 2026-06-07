import { LitElement, html, css } from "lit";
import { customElement, property } from "lit/decorators.js";
import type { Game } from "../../ipc";
import "./game-tile";

export type Density = "comfortable" | "compact";

/**
 * Layout of [`game-tile`]s. `density="comfortable"` is the responsive cover grid
 * (auto-fill ≈ 240px tiles). `density="compact"` is a single game per row (list
 * view) — tiles render their compact row layout. Pure presentation: it renders
 * the `games` it's given; the container (`library-view`) owns fetching/scanning.
 */
@customElement("game-grid")
export class GameGrid extends LitElement {
  static styles = css`
    :host {
      display: block;
    }
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
      gap: 24px;
      /* Comfortable: 5 cards per row, stepping down on narrower widths. */
      grid-template-columns: repeat(5, 1fr);
    }
    @media (max-width: 1400px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: repeat(4, 1fr);
      }
    }
    @media (max-width: 1100px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: repeat(3, 1fr);
      }
    }
    @media (max-width: 760px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: repeat(2, 1fr);
      }
    }
    @media (max-width: 480px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: 1fr;
      }
    }
    /* Compact = a single game per row (list view), not tiny tiles. */
    :host([density="compact"]) ul {
      gap: 8px;
      grid-template-columns: 1fr;
    }
  `;

  @property({ attribute: false }) games: Game[] = [];
  /** Tile size; reflected to the host so CSS can switch on it. */
  @property({ reflect: true }) density: Density = "comfortable";

  render() {
    const list = this.density === "compact";
    return html`
      <ul role="list">
        ${this.games.map(
          (game) => html`
            <li role="listitem">
              <game-tile .game=${game} ?list=${list}></game-tile>
            </li>
          `,
        )}
      </ul>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "game-grid": GameGrid;
  }
}
